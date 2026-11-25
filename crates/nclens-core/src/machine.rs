use std::collections::HashMap;

use crate::cutter_comp::{self, CutterComp, Mode};
use crate::syntax::Result;
use crate::tool::Catalog;
use crate::{
    CodeSettings, CompensationType, CoordinateBounds, MacroVariables, ModalSnapshot, MotionTotals,
    PostProfile, RunResult, SegKind, Segment, ToolChange,
};

pub type Words = [(u8, f64)];
const AXES: [u8; 3] = *b"XYZ";

pub fn word(words: &Words, addr: u8) -> Option<f64> {
    words.iter().rev().find(|w| w.0 == addr).map(|w| w.1)
}

use crate::dialect::{semantic_group as group, DWELL_FEED, DYNAMIC_LENGTH, WORK_SELECT_H};

pub fn offset_number(group: u32, slot: u32) -> Option<u32> {
    if group > 3 || !(1..=999).contains(&slot) {
        return None;
    }
    Some(if slot <= 200 {
        2000 + group * 200 + slot
    } else {
        10000 + group * 1000 + slot
    })
}

pub fn canonical_offset(number: u32) -> Option<u32> {
    match number {
        2001..=2800 => Some(number),
        10001..=13999 => offset_number((number - 10000) / 1000, number % 1000),
        _ => None,
    }
}

struct Cycle {
    initial: f64,
    r: Option<f64>,
    depth: Option<f64>,
    q: f64,
}

pub struct Machine<'a> {
    codes: &'a CodeSettings,
    reference: [[f64; 3]; 4],
    rpm_limit: f64,
    modal: HashMap<u32, f64>,
    pub offsets: HashMap<u32, Option<f64>>,
    work: [[f64; 3]; 7],
    local: [f64; 3],
    rotation: Option<([f64; 2], f64)>,
    position: [f64; 3],
    block_end: [f64; 3],
    length: f64,
    dynamic_length: Option<f64>,
    tools: Catalog<'a>,
    cutter_comp: CutterComp,
    measure_compensated_path: bool,
    selected_tool: u32,
    active_tool: Option<u32>,
    cycle: Option<Cycle>,
    rotary: [f64; 3],
    auxiliary: [f64; 3],
    work_selected_h: bool,
    pub program_name: String,
}

impl<'a> Machine<'a> {
    pub fn new(post: &'a PostProfile, src: &str) -> Self {
        let modal = [
            (4001, 0.),
            (4002, 17.),
            (4003, 90.),
            (4005, 94.),
            (4006, 21.),
            (4007, 40.),
            (4008, 49.),
            (4010, 98.),
            (4012, 54.),
            (4014, 67.),
            (4016, 69.),
            (4017, 97.),
        ]
        .into_iter()
        .collect();
        let mut machine = Self {
            codes: &post.codes,
            rpm_limit: f64::INFINITY,
            reference: std::array::from_fn(|i| {
                if i == 0 {
                    post.variables.g28
                } else {
                    post.variables.g30[i - 1]
                }
            }),
            modal,
            offsets: HashMap::new(),
            work: [[0.; 3]; 7],
            local: [0.; 3],
            rotation: None,
            position: [0.; 3],
            block_end: [0.; 3],
            length: 0.,
            dynamic_length: None,
            tools: Catalog::new(post, src),
            cutter_comp: CutterComp::new(post.codes.measure_compensated_path),
            measure_compensated_path: post.codes.measure_compensated_path,
            selected_tool: 0,
            active_tool: None,
            cycle: None,
            rotary: [0.; 3],
            auxiliary: [0.; 3],
            work_selected_h: false,
            program_name: String::new(),
        };
        machine.work[0] = post.variables.external;
        machine.work[1..].copy_from_slice(&post.variables.work);
        for (number, offset) in &post.variables.offsets {
            let Ok(slot) = number.parse() else { continue };
            for (group, value) in [
                offset.h_geometry,
                offset.h_wear,
                offset.d_geometry,
                offset.d_wear,
            ]
            .into_iter()
            .enumerate()
            {
                if let Some(n) = offset_number(group as u32, slot) {
                    machine.offsets.insert(n, Some(value));
                }
            }
        }
        machine
    }

    pub fn finish(&mut self, result: &mut RunResult) {
        self.tools.finish(result);
        self.cutter_comp.finish(result);
    }

    fn d_radius(&self, slot: u32) -> Option<f64> {
        let groups: &[u32] = match self.codes.compensation {
            CompensationType::A => &[0],
            CompensationType::B => &[2],
            CompensationType::C => &[2, 3],
        };
        groups
            .iter()
            .filter_map(|&g| offset_number(g, slot))
            .filter_map(|n| self.offsets.get(&n).copied().flatten())
            .reduce(|geometry, wear| geometry + wear)
    }

    pub fn modal_codes(&self) -> impl Iterator<Item = f64> + '_ {
        (4001..4100).filter_map(|id| self.modal.get(&id).copied())
    }

    pub fn snapshot(&self) -> ModalSnapshot {
        ModalSnapshot {
            g_codes: Default::default(),
            feed: self.modal(4109),
            spindle: self.modal(4119),
            tool: self.selected_tool,
            h: self.modal(4111) as u32,
            d: self.modal(4107) as u32,
            work_coordinate: if self.work_selected_h {
                f64::from(WORK_SELECT_H)
            } else {
                self.modal(4012)
            },
            plane: self.modal(4002),
            absolute: self.modal(4003) != 91.,
            position: self.with_rotary(self.work_position()),
        }
    }

    pub fn with_rotary(&self, [x, y, z]: [f64; 3]) -> [f64; 6] {
        let [a, b, c] = self.rotary;
        [x, y, z, a, b, c]
    }

    fn record_point(&self, result: &mut RunResult, line: usize) {
        let work = self.with_rotary(self.work_position());
        let machine = self.with_rotary(self.position);
        let at = || CoordinateBounds::at(work, machine);
        let tool = self.active_tool.unwrap_or(self.selected_tool);
        result.bounds.include(work, machine);
        result
            .tool_bounds
            .entry(tool)
            .or_insert_with(at)
            .include(work, machine);
        let tool_report =
            result
                .report
                .tools
                .entry(tool)
                .or_insert_with(|| crate::ReportBreakdown {
                    bounds: at(),
                    ..Default::default()
                });
        tool_report.bounds.include(work, machine);
        tool_report.used_lines.insert(line);
        self.record_program_point(result, line, work, machine);
    }

    fn record_program_point(
        &self,
        result: &mut RunResult,
        line: usize,
        work: [f64; 6],
        machine: [f64; 6],
    ) {
        let program_report = result
            .report
            .programs
            .entry(self.program_name.clone())
            .or_insert_with(|| crate::ReportBreakdown {
                bounds: CoordinateBounds::at(work, machine),
                ..Default::default()
            });
        program_report.bounds.include(work, machine);
        program_report.used_lines.insert(line);
    }

    pub fn account_program_line(&self, result: &mut RunResult, line: usize) {
        self.record_program_point(
            result,
            line,
            self.with_rotary(self.work_position()),
            self.with_rotary(self.position),
        );
    }

    pub fn initialize_bounds(&self, result: &mut RunResult) {
        result.bounds = CoordinateBounds::at(
            self.with_rotary(self.work_position()),
            self.with_rotary(self.position),
        );
    }

    fn totals<'r>(&self, result: &'r mut RunResult) -> [&'r mut MotionTotals; 3] {
        let tool = self.active_tool.unwrap_or(self.selected_tool);
        [
            &mut result.report.totals,
            &mut result.report.tools.get_mut(&tool).unwrap().totals,
            &mut result
                .report
                .programs
                .get_mut(&self.program_name)
                .unwrap()
                .totals,
        ]
    }

    pub fn account_block(&self, result: &mut RunResult, line: usize) {
        self.record_point(result, line);
    }

    fn record_other_time(&self, result: &mut RunResult, line: usize, time: f64) {
        if time > 0. {
            result.stats.time_sec += time;
            self.record_point(result, line);
            for totals in self.totals(result) {
                totals.other_time_sec += time;
            }
        }
    }

    fn tool_offset_address(&self, var: u32) -> Option<u32> {
        let canonical = canonical_offset(var)?;
        // The extended registers expose all four stored fields.
        if var >= 10000 {
            return Some(canonical);
        }
        let (group, slot) = ((canonical - 2001) / 200, (canonical - 2001) % 200 + 1);
        let group = match (self.codes.compensation, group) {
            (CompensationType::A, 0) => 0,
            (CompensationType::B, 0) => 0,
            (CompensationType::B, 1) => 2,
            (CompensationType::C, g) => g,
            _ => return None,
        };
        offset_number(
            if self.codes.macro_variables == MacroVariables::M16 {
                group ^ 1
            } else {
                group
            },
            slot,
        )
    }

    pub fn modal(&self, var: u32) -> f64 {
        self.modal.get(&var).copied().unwrap_or_default()
    }

    pub fn work_address(var: u32) -> Option<(usize, usize)> {
        let offset = var.checked_sub(5201)?;
        (offset / 20 <= 6 && offset % 20 < 3)
            .then_some(((offset / 20) as usize, (offset % 20) as usize))
    }

    pub fn read(&self, var: u32) -> Option<f64> {
        Some(match var {
            4001..=4130 => self.modal(var),
            5001..=5003 => self.block_end[(var - 5001) as usize],
            5021..=5023 => self.position[(var - 5021) as usize],
            5041..=5043 => self.work_position()[(var - 5041) as usize],
            _ => {
                if let Some(n) = self.tool_offset_address(var) {
                    return Some(self.offsets.get(&n).copied().flatten().unwrap_or_default());
                }
                let (slot, axis) = Self::work_address(var)?;
                self.work[slot][axis]
            }
        })
    }

    pub fn write(&mut self, var: u32, value: Option<f64>) -> bool {
        if let Some(n) = self.tool_offset_address(var) {
            self.offsets.insert(n, value);
        } else if let Some((slot, axis)) = Self::work_address(var) {
            self.work[slot][axis] = value.unwrap_or_default();
        } else {
            return false;
        }
        true
    }

    fn wcs(&self) -> usize {
        (self.modal(4012) as usize - 53).min(6)
    }

    fn origin(&self) -> [f64; 3] {
        std::array::from_fn(|a| self.work[0][a] + self.work[self.wcs()][a] + self.local[a])
    }

    fn rotate(&self, point: [f64; 3], inverse: bool) -> [f64; 3] {
        if let Some(([x, y], angle)) = self.rotation {
            let (sin, cos) = if inverse { -angle } else { angle }.sin_cos();
            let dx = point[0] - x;
            let dy = point[1] - y;
            [x + dx * cos - dy * sin, y + dx * sin + dy * cos, point[2]]
        } else {
            point
        }
    }

    pub fn work_position(&self) -> [f64; 3] {
        self.work_at(self.position)
    }

    pub fn work_at(&self, machine: [f64; 3]) -> [f64; 3] {
        let origin = self.origin();
        let mut point = std::array::from_fn(|a| machine[a] - origin[a]);
        point[2] -= self.length;
        self.rotate(point, true)
    }

    pub fn machine_position(&self) -> [f64; 3] {
        self.position
    }

    pub fn output_position(&self, point: [f64; 3]) -> [f64; 3] {
        let work = self.work[self.wcs()];
        std::array::from_fn(|a| {
            point[a] - self.work[0][a] - work[a] - if a == 2 { self.length } else { 0. }
        })
    }

    pub fn g10(&mut self, words: &Words) -> Result<Vec<(u32, Option<f64>)>> {
        let l = word(words, b'L').ok_or("Missing G10 L")? as u32;
        let p = word(words, b'P').ok_or("Missing G10 P")? as u32;
        let mut changes = Vec::new();
        if l == 2 && p <= 6 {
            for (axis, addr) in AXES.into_iter().enumerate() {
                if let Some(value) = word(words, addr) {
                    changes.push((5201 + p * 20 + axis as u32, Some(value)));
                }
            }
        } else {
            let group = match (self.codes.compensation, l) {
                (CompensationType::A, 11) | (CompensationType::B, 10) => 0,
                (CompensationType::B, 11) => 2,
                (CompensationType::C, 10..=13) => l - 10,
                _ => return Err("Unsupported G10 L/P".into()),
            };
            let n = offset_number(group, p).ok_or("Invalid G10 tool offset")?;
            let mut value = word(words, b'R').ok_or("Missing G10 R")?;
            if self.modal(4003) == 91. {
                value += self.offsets.get(&n).copied().flatten().unwrap_or_default();
            }
            self.offsets.insert(n, Some(value));
            let extended = 10000 + group * 1000 + p;
            let var = if p <= 200 {
                (0..4)
                    .map(|g| 2000 + g * 200 + p)
                    .find(|&var| self.tool_offset_address(var) == Some(n))
                    .unwrap_or(extended)
            } else {
                extended
            };
            return Ok(vec![(var, Some(value))]);
        }
        for (n, v) in &mut changes {
            if self.modal(4003) == 91. {
                *v = Some(v.unwrap_or_default() + self.read(*n).unwrap_or_default());
            }
            self.write(*n, *v);
        }
        Ok(changes)
    }

    pub fn block(
        &mut self,
        words: &mut Vec<(u8, f64)>,
        line: usize,
        result: &mut RunResult,
    ) -> Result<bool> {
        let start = result.path.len();
        let supported = !words.iter().any(|&(a, value)| {
            (a == b'G' && [28., 30., 53.].contains(&value)) || b"ABCUVW".contains(&a)
        });
        let d_command = word(words, b'D').is_some() && !words.contains(&(b'G', 10.));
        let moved = self.execute_block(words, line, result);
        let plane = match self.modal(4002) as u32 {
            18 => [2, 0, 1],
            19 => [1, 2, 0],
            _ => [0, 1, 2],
        };
        let d = self.modal(4107);
        let valid_d = d.fract() == 0. && (1. ..=999.).contains(&d);
        if valid_d
            && (d_command || matches!(self.modal(4007), 41. | 42.))
            && (d_command || start != result.path.len())
        {
            let tool = self.active_tool.unwrap_or(self.selected_tool);
            self.tools
                .observe(tool, line, self.d_radius(d as u32), true);
        }
        let rotated_arc = self.rotation.is_some() && self.cutter_comp.arc.is_some();
        let mode = match self.modal(4007) as u32 {
            41 | 42 if d == 0. && self.modal.contains_key(&4107) => Mode::Off,
            side @ (41 | 42) if valid_d => match self.d_radius(d as u32) {
                Some(0.) => Mode::Off,
                Some(radius) if radius.is_finite() => {
                    Mode::Offset(if side == 41 { radius } else { -radius })
                }
                _ => Mode::Missing,
            },
            41 | 42 => Mode::Missing,
            _ => Mode::Off,
        };
        self.cutter_comp.record(
            result,
            cutter_comp::Block {
                line,
                range: start..result.path.len(),
                plane,
                mode,
                supported: supported && moved.is_ok() && self.modal(4001) <= 3. && !rotated_arc,
                program: if self.measure_compensated_path && start != result.path.len() {
                    self.program_name.clone()
                } else {
                    String::new()
                },
            },
        );
        moved
    }

    fn execute_block(
        &mut self,
        words: &mut Vec<(u8, f64)>,
        line: usize,
        result: &mut RunResult,
    ) -> Result<bool> {
        let mut codes = Vec::new();
        for &(a, v) in words.iter() {
            if a == b'G' && v.fract() == 0. {
                codes.push(v as u32);
            }
        }

        let (u, v, w) = match codes
            .iter()
            .rev()
            .copied()
            .find(|g| (17..=19).contains(g))
            .unwrap_or(self.modal(4002) as u32)
        {
            18 => (2, 0, 1),
            19 => (1, 2, 0),
            _ => (0, 1, 2),
        };
        let explicit_motion = codes
            .iter()
            .rev()
            .copied()
            .find(|g| *g != 80 && group(*g) == Some(1))
            .or_else(|| codes.contains(&80).then_some(80));
        let initial = self.work_position()[w];
        for &g in &codes {
            if let Some(id) = group(g) {
                if id == 12 {
                    self.work_selected_h = false;
                }
                if id != 1 {
                    self.modal.insert(4000 + id, g as f64);
                }
            }
        }
        if let Some(motion) = explicit_motion {
            self.modal.insert(4001, motion as f64);
        }
        for (a, id) in [
            (b'D', 4107),
            (b'F', 4109),
            (b'H', 4111),
            (b'M', 4113),
            (b'P', 4116),
            (b'Q', 4117),
            (b'R', 4118),
            (b'S', 4119),
            (b'T', 4120),
        ] {
            if let Some(value) = word(words, a) {
                if a == b'F' && codes.contains(&DWELL_FEED) {
                    continue;
                }
                if a == b'H' && codes.contains(&WORK_SELECT_H) {
                    continue;
                }
                if a == b'S' && codes.contains(&50) {
                    self.rpm_limit = if value > 0. { value } else { f64::INFINITY };
                    continue;
                }
                self.modal.insert(id, value);
            }
        }
        if let Some(t) = word(words, b'T') {
            self.selected_tool = t as u32;
            self.tools.select(self.selected_tool, line);
            self.tools.observe(
                self.selected_tool,
                line,
                self.d_radius(self.selected_tool),
                false,
            );
        }
        let tool_change = match self.codes.tool_change {
            ToolChange::T => word(words, b'T').is_some(),
            ToolChange::M6 => words.contains(&(b'M', 6.)),
        };
        if tool_change && self.active_tool != Some(self.selected_tool) {
            self.active_tool = Some(self.selected_tool);
            self.tools.index(result, self.selected_tool);
            self.record_other_time(result, line, self.codes.atc_sec);
        }
        if codes.iter().any(|code| matches!(code, 43 | 44 | 49)) {
            self.dynamic_length = None;
        }
        if codes.contains(&WORK_SELECT_H) {
            let h = word(words, b'H').ok_or("Missing work coordinate H")?;
            if h.fract() != 0. || !(1. ..=6.).contains(&h) {
                return Err("Work coordinate H must be between 1 and 6".into());
            }
            self.modal.insert(4012, 53. + h);
            self.work_selected_h = true;
        }
        if codes.contains(&DYNAMIC_LENGTH) {
            self.dynamic_length = Some(word(words, b'Z').ok_or("Missing dynamic tool length Z")?);
            self.modal.insert(4008, f64::from(DYNAMIC_LENGTH));
        }
        if let Some(length) = self.dynamic_length {
            self.length = length;
        } else if self.modal(4008) == 49. {
            self.length = 0.;
        } else {
            let slot = self.modal(4111) as u32;
            let groups = if self.codes.compensation == CompensationType::A {
                &[0][..]
            } else {
                &[0, 1][..]
            };
            let length = groups
                .iter()
                .copied()
                .filter_map(|g| offset_number(g, slot))
                .filter_map(|n| self.offsets.get(&n).copied().flatten())
                .sum::<f64>();
            self.length = if self.modal(4008) == 44. {
                -length
            } else {
                length
            };
        }
        if codes.contains(&DWELL_FEED) {
            self.record_other_time(result, line, word(words, b'F').unwrap_or_default());
            return Ok(false);
        }
        if let Some(feed) = word(words, b'F') {
            let report = &mut result.report;
            report.feed_min = Some(report.feed_min.map_or(feed, |v| v.min(feed)));
            report.feed_max = Some(report.feed_max.map_or(feed, |v| v.max(feed)));
        }
        if codes.contains(&4) {
            let seconds = word(words, b'P')
                .map(|p| p / 1000.)
                .or_else(|| word(words, b'X'))
                .or_else(|| word(words, b'U'))
                .unwrap_or_default()
                .max(0.);
            self.record_other_time(result, line, seconds);
            return Ok(false);
        }
        if codes.contains(&DYNAMIC_LENGTH) {
            return Ok(false);
        }
        if codes.contains(&10) {
            for (var, value) in self.g10(words)? {
                result.var_trace.push(crate::VarChange {
                    src_line: line,
                    var,
                    value,
                });
            }
            return Ok(false);
        }
        if codes.contains(&52) {
            for (axis, addr) in AXES.into_iter().enumerate() {
                if let Some(value) = word(words, addr) {
                    self.local[axis] = value;
                }
            }
            words.retain(|(a, value)| !AXES.contains(a) && (*a, *value) != (b'G', 52.));
            return Ok(false);
        }
        if codes.contains(&68) || codes.contains(&69) {
            let at = self.work_position();
            self.rotation = if codes.contains(&69) {
                None
            } else {
                Some((
                    [
                        word(words, b'X').unwrap_or(at[0]),
                        word(words, b'Y').unwrap_or(at[1]),
                    ],
                    word(words, b'R').ok_or("Missing G68 R")?.to_radians(),
                ))
            };
            words.retain(|(a, value)| {
                !b"XYR".contains(a) && !(*a == b'G' && [68., 69.].contains(value))
            });
            return Ok(false);
        }

        let motion = self.modal(4001) as u32;
        let incremental = self.modal(4003) == 91.;
        let specified = AXES.map(|a| word(words, a));
        let has_axis = specified.iter().any(Option::is_some);
        let rotary = b"ABC".map(|a| word(words, a));
        let rotary_move = motion <= 3 && rotary.iter().any(Option::is_some);
        if rotary_move {
            self.record_point(result, line);
            for (axis, value) in rotary.into_iter().enumerate() {
                if let Some(value) = value {
                    self.rotary[axis] = value
                        + if incremental && !codes.contains(&53) {
                            self.rotary[axis]
                        } else {
                            0.
                        };
                }
            }
        }
        if codes.contains(&28) || codes.contains(&30) {
            let reference = if codes.contains(&28) {
                self.reference[0]
            } else {
                let p = word(words, b'P').unwrap_or(2.);
                if ![2., 3., 4.].contains(&p) {
                    return Err("Invalid G30 P".into());
                }
                self.reference[p as usize - 1]
            };
            let current = self.work_position();
            let point = self.rotate(
                std::array::from_fn(|a| match specified[a] {
                    Some(v) if incremental => current[a] + v,
                    Some(v) => v,
                    None => current[a],
                }),
                false,
            );
            let origin = self.origin();
            let intermediate = std::array::from_fn(|a| {
                point[a] + origin[a] + if a == 2 { self.length } else { 0. }
            });
            if has_axis {
                self.segment(result, line, intermediate, SegKind::Rapid, None);
            }
            let end = std::array::from_fn(|a| {
                if !has_axis || specified[a].is_some() {
                    reference[a]
                } else {
                    self.position[a]
                }
            });
            self.segment(result, line, end, SegKind::Rapid, None);
            self.block_end = self.work_position();
            return Ok(true);
        }
        if motion == 73 || (81..=89).contains(&motion) {
            return self.drill(
                words,
                line,
                result,
                motion,
                w,
                initial,
                incremental,
                has_axis,
                explicit_motion.is_some(),
            );
        }
        self.cycle = None;
        let mut next_aux = self.auxiliary;
        if motion < 2 {
            for (i, addr) in b"UVW".iter().copied().enumerate() {
                if let Some(value) = word(words, addr) {
                    next_aux[i] = value
                        + if incremental && !codes.contains(&53) {
                            self.auxiliary[i]
                        } else {
                            0.
                        };
                }
            }
        }
        let auxiliary_distance = next_aux
            .iter()
            .zip(self.auxiliary)
            .map(|(to, from)| (to - from).powi(2))
            .sum::<f64>()
            .sqrt();
        let has_center = b"IJKR".iter().any(|&a| word(words, a).is_some());
        if motion > 3 || !(has_axis || auxiliary_distance > 0. || (motion >= 2 && has_center)) {
            return Ok(rotary_move);
        }

        let current = self.work_position();
        let machine = codes.contains(&53);
        let target = std::array::from_fn(|a| match specified[a] {
            Some(value) if incremental && !machine => current[a] + value,
            Some(value) => value,
            None if machine => self.position[a],
            None => current[a],
        });
        let origin = self.origin();
        let rotated = self.rotate(target, false);
        let to = if machine {
            target
        } else {
            std::array::from_fn(|a| rotated[a] + origin[a] + if a == 2 { self.length } else { 0. })
        };

        if !machine && !incremental {
            for (a, value) in words.iter_mut() {
                if let Some(axis) = AXES.iter().position(|addr| addr == a) {
                    *value = rotated[axis] + self.local[axis];
                }
            }
            if self.rotation.is_some() && (specified[0].is_some() || specified[1].is_some()) {
                for (axis, a) in b"XY".iter().copied().enumerate() {
                    if specified[axis].is_none() {
                        words.push((a, rotated[axis] + self.local[axis]));
                    }
                }
                if specified[0].is_none() {
                    let n = words.len();
                    let y = words.iter().position(|w| w.0 == b'Y').unwrap();
                    words[y..n].rotate_right(1);
                }
            }
        }
        if motion >= 2 {
            self.arc(words, line, result, to, [u, v, w], motion == 2)?;
        } else {
            self.segment(
                result,
                line,
                to,
                if motion == 0 {
                    SegKind::Rapid
                } else {
                    SegKind::Cut
                },
                (auxiliary_distance > 0.).then(|| {
                    let xyz = to
                        .iter()
                        .zip(self.position)
                        .map(|(to, from)| (to - from).powi(2))
                        .sum::<f64>()
                        .sqrt();
                    xyz.hypot(auxiliary_distance)
                }),
            );
            self.auxiliary = next_aux;
        }
        self.block_end = self.work_position();

        Ok(true)
    }

    fn segment(
        &mut self,
        result: &mut RunResult,
        line: usize,
        to: [f64; 3],
        kind: SegKind,
        exact: Option<f64>,
    ) {
        let from = self.position;
        let distance = exact.unwrap_or_else(|| {
            (0..3)
                .map(|a| (to[a] - from[a]).powi(2))
                .sum::<f64>()
                .sqrt()
        });
        if distance == 0. {
            return;
        }
        self.record_point(result, line);
        self.position = to;
        self.record_point(result, line);
        if result.path.is_empty() {
            result.stats.min = from;
            result.stats.max = from;
        }
        for (a, &end) in to.iter().enumerate() {
            result.stats.min[a] = result.stats.min[a].min(end);
            result.stats.max[a] = result.stats.max[a].max(end);
        }
        let speed = match kind {
            SegKind::Rapid => {
                result.stats.rapid_length_mm += distance;
                self.codes.rapid_mm_min
            }
            SegKind::Cut => {
                result.stats.cut_length_mm += distance;
                self.modal(4109)
                    * if self.modal(4005) == 95. {
                        self.spindle_rpm(from, to)
                    } else {
                        1.
                    }
            }
        };
        let time_sec = if speed > 0. {
            60. * distance / speed
        } else {
            0.
        };
        result.stats.time_sec += time_sec;
        for totals in self.totals(result) {
            match kind {
                SegKind::Rapid => {
                    totals.rapid_length_mm += distance;
                    totals.rapid_time_sec += time_sec;
                }
                SegKind::Cut => {
                    totals.cut_length_mm += distance;
                    totals.cut_time_sec += time_sec;
                }
            }
        }
        let number = self.active_tool.unwrap_or(self.selected_tool);
        self.tools
            .observe(number, line, self.d_radius(number), false);
        let tool_index = self.tools.index(result, number);
        result.path.push(Segment {
            kind,
            from,
            to,
            comp_from: None,
            comp_to: None,
            src_line: line,
            tool_index,
            time_sec,
            // ブロックの展開行は block() の後に積まれるので、次に積まれる添字
            expanded_index: result.expanded.len(),
        });
    }

    fn spindle_rpm(&self, from: [f64; 3], to: [f64; 3]) -> f64 {
        let s = self.modal(4119);
        let configured_max = if self.codes.max_rpm > 0. {
            self.codes.max_rpm
        } else {
            f64::INFINITY
        };
        let rpm = if self.modal(4017) == 96. && s > 0. {
            let diameter =
                ((self.output_position(from)[0] + self.output_position(to)[0]) / 2.).abs();
            1000. * s / (std::f64::consts::PI * diameter)
        } else {
            s
        };
        let capped = rpm.min(self.rpm_limit).min(configured_max);
        if capped.is_finite() {
            capped
        } else {
            0.
        }
    }

    fn arc(
        &mut self,
        words: &mut Vec<(u8, f64)>,
        line: usize,
        result: &mut RunResult,
        to: [f64; 3],
        [u, v, w]: [usize; 3],
        cw: bool,
    ) -> Result<()> {
        let from = self.position;
        let delta = [to[u] - from[u], to[v] - from[v]];
        let chord = delta[0].hypot(delta[1]);
        let center = if let Some(r) = word(words, b'R') {
            let h2 = r * r - chord * chord / 4.;
            if chord == 0. || h2 < -1e-8 {
                return Err("Invalid radius arc".into());
            }
            let sign = if cw == (r > 0.) { -1. } else { 1. };
            let h = sign * h2.max(0.).sqrt() / chord;
            [
                from[u] + delta[0] / 2. - h * delta[1],
                from[v] + delta[1] / 2. + h * delta[0],
            ]
        } else {
            let addresses = *b"IJK";
            let a = word(words, addresses[u]);
            let b = word(words, addresses[v]);
            if a.is_none() && b.is_none() {
                return Err("Missing arc center".into());
            }
            if chord < 1e-9 {
                for (addr, value) in [(addresses[u], a), (addresses[v], b)] {
                    if value.is_none() {
                        words.push((addr, 0.));
                    }
                }
            }
            [
                from[u] + a.unwrap_or_default(),
                from[v] + b.unwrap_or_default(),
            ]
        };
        let start = (from[v] - center[1]).atan2(from[u] - center[0]);
        let end = (to[v] - center[1]).atan2(to[u] - center[0]);
        let radius = (from[u] - center[0]).hypot(from[v] - center[1]);
        let end_radius = (to[u] - center[0]).hypot(to[v] - center[1]);
        if !radius.is_finite()
            || !end_radius.is_finite()
            || radius == 0.
            || (radius - end_radius).abs() > 0.002
        {
            return Err("Arc radii differ".into());
        }
        let tau = std::f64::consts::TAU;
        let mut sweep = if cw {
            (start - end).rem_euclid(tau)
        } else {
            (end - start).rem_euclid(tau)
        };
        if chord < 1e-9 {
            sweep = tau;
        }
        self.cutter_comp.arc = Some(cutter_comp::Arc {
            center,
            start,
            sweep: if cw { -sweep } else { sweep },
            radius,
        });
        let count = (sweep.to_degrees() / 5.).ceil().max(1.) as usize;
        let length = (radius * sweep).hypot(to[w] - from[w]);
        let mut fractions: Vec<_> = (1..=count).map(|i| i as f64 / count as f64).collect();
        for quadrant in 0..4 {
            let angle = quadrant as f64 * std::f64::consts::FRAC_PI_2;
            let delta = if cw { start - angle } else { angle - start }.rem_euclid(tau);
            if delta > 0. && delta < sweep {
                fractions.push(delta / sweep);
            }
        }
        fractions.sort_unstable_by(f64::total_cmp);
        fractions.dedup_by(|a, b| (*a - *b).abs() < 1e-14 && *a != 1. && *b != 1.);
        let mut previous = 0.;
        for fraction in fractions {
            let (sin, cos) = (start + if cw { -sweep } else { sweep } * fraction).sin_cos();
            let mut next = to;
            if fraction < 1. {
                next[u] = center[0] + radius * cos;
                next[v] = center[1] + radius * sin;
                next[w] = from[w] + (to[w] - from[w]) * fraction;
            }
            self.segment(
                result,
                line,
                next,
                SegKind::Cut,
                Some(length * (fraction - previous)),
            );
            previous = fraction;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn drill(
        &mut self,
        words: &Words,
        line: usize,
        result: &mut RunResult,
        motion: u32,
        w: usize,
        initial: f64,
        incremental: bool,
        has_axis: bool,
        explicit: bool,
    ) -> Result<bool> {
        let cycle = self.cycle.get_or_insert(Cycle {
            initial,
            r: None,
            depth: None,
            q: 0.,
        });
        if let Some(r) = word(words, b'R') {
            cycle.r = Some(if incremental { cycle.initial + r } else { r });
        }
        if let Some(z) = word(words, AXES[w]) {
            cycle.depth = Some(if incremental {
                cycle.r.ok_or("Missing cycle R")? + z
            } else {
                z
            });
        }
        if let Some(q) = word(words, b'Q') {
            cycle.q = q.abs();
        }
        if !has_axis && !explicit {
            return Ok(false);
        }
        let r = cycle.r.ok_or("Missing cycle R")?;
        let depth = cycle.depth.ok_or("Missing cycle depth")?;
        let end = if self.modal.get(&4010) == Some(&99.) {
            r
        } else {
            cycle.initial
        };
        let q = cycle.q;
        let repeats = word(words, b'K')
            .or_else(|| word(words, b'L'))
            .unwrap_or(1.) as usize;
        let pecks = if matches!(motion, 73 | 83) && q > 0. {
            ((depth - r).abs() / q).ceil() as usize
        } else {
            1
        };
        if repeats.saturating_mul(pecks.max(1)) > 10000 {
            return Err("Cycle move limit".into());
        }
        let origin = self.origin();
        let shift = origin[w] + if w == 2 { self.length } else { 0. };
        let direction = (depth - r).signum();
        for _ in 0..repeats {
            if matches!(motion, 73 | 83) {
                let lines = 5 + pecks.saturating_sub(1) * if motion == 73 { 2 } else { 3 };
                self.record_other_time(result, line, self.codes.compensation_sec * lines as f64);
            }
            let current = self.work_position();
            let target = self.rotate(
                std::array::from_fn(|a| {
                    if a == w {
                        current[a]
                    } else {
                        word(words, AXES[a]).map_or(current[a], |v| {
                            if incremental {
                                current[a] + v
                            } else {
                                v
                            }
                        })
                    }
                }),
                false,
            );
            let mut to = std::array::from_fn(|a| {
                target[a] + origin[a] + if a == 2 { self.length } else { 0. }
            });
            self.segment(result, line, to, SegKind::Rapid, None);
            to[w] = r + shift;
            self.segment(result, line, to, SegKind::Rapid, None);
            for i in 1..pecks {
                let d = r + direction * q * i as f64;
                to[w] = d + shift;
                self.segment(result, line, to, SegKind::Cut, None);
                if motion == 83 {
                    to[w] = r + shift;
                    self.segment(result, line, to, SegKind::Rapid, None);
                }
                let retract = if motion == 73 {
                    self.codes.g73_retract_mm
                } else {
                    self.codes.g83_clearance_mm
                };
                to[w] = d - direction * retract.min((d - r).abs()) + shift;
                self.segment(result, line, to, SegKind::Rapid, None);
            }
            to[w] = depth + shift;
            self.segment(result, line, to, SegKind::Cut, None);
            // The documented reference geometry uses a rapid return for G81–G89.
            to[w] = end + shift;
            self.segment(result, line, to, SegKind::Rapid, None);
        }
        self.block_end = self.work_position();
        Ok(repeats > 0)
    }
}
