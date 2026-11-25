use std::collections::HashMap;

use crate::dialect::{Action, Dialect, DWELL_FEED, DYNAMIC_LENGTH};
use crate::duplicate::{retained_words, EvaluatedWord, SourceWord};
use crate::machine::{word, Machine};
use crate::syntax::{self, Body, Expr, Op, Program, Result, Word, OSP_COMMON_BASE, OSP_LOCAL_BASE};
use crate::{
    CallEvent, CallKind, Diagnostic, DistanceMode, ExpandedLine, IntegerUnits, PostProfile,
    RunResult, SegKind, StopKind, StopPoint, VarChange,
};

#[derive(Clone)]
struct Call {
    program: u32,
    repeats: usize,
    locals: Option<Locals>,
}

#[derive(Clone)]
struct Locals {
    values: [Option<f64>; 33],
    implicit_decimal: [bool; 33],
    named: std::collections::BTreeMap<u32, Option<f64>>,
}

impl Default for Locals {
    fn default() -> Self {
        Self {
            values: [None; 33],
            implicit_decimal: [false; 33],
            named: Default::default(),
        }
    }
}

struct Frame {
    program: usize,
    program_number: u32,
    pc: usize,
    repeat: usize,
    locals: usize,
    owns_locals: bool,
    site: Option<String>,
    modal: Option<Call>,
    in_modal_call: bool,
    pending_args: bool,
}

struct Engine<'a> {
    post: &'a PostProfile,
    dialect: Dialect,
    source_words: Vec<SourceWord>,
    source_modal: std::collections::BTreeMap<u32, (f64, f64)>,
    frames: Vec<Frame>,
    locals: Vec<Locals>,
    common: HashMap<u32, Option<f64>>,
    machine: Machine<'a>,
    result: RunResult,
    line: usize,
    last_distance: Option<f64>,
    timers: [f64; 2],
    stopped: bool,
    optional_stop: bool,
    through_stops: bool,
    last_label: u32,
}

type Trace<'a> = &'a mut dyn FnMut(usize, &[String], &[VarChange]);

pub fn run(
    src: &str,
    post: &PostProfile,
    levels: &[u8],
    optional_stop: bool,
    through_stops: bool,
    mut trace: Option<Trace<'_>>,
) -> RunResult {
    let mut result = RunResult::default();
    let dialect = Dialect::new(post);
    let programs = syntax::parse(
        src,
        dialect.program_address,
        dialect.sequence_address,
        &dialect.macro_syntax,
        &mut result,
    );
    let block_skip = levels
        .iter()
        .filter(|n| (1..=9).contains(*n))
        .fold(0, |mask, &n| mask | (1u16 << (n - 1)));
    // Keep the parsed program immutable while mutating the execution state.
    let machine = Machine::new(post, src);
    let source_modal = machine
        .modal_codes()
        .filter_map(|canonical| {
            dialect
                .modal_code(canonical)
                .map(|(group, source)| (group, (canonical, source)))
        })
        .collect();
    let mut engine = Engine {
        post,
        dialect,
        source_words: Vec::new(),
        source_modal,
        frames: vec![Frame {
            program: 0,
            program_number: programs[0].number,
            pc: 0,
            repeat: 1,
            locals: 0,
            owns_locals: false,
            site: None,
            modal: None,
            in_modal_call: false,
            pending_args: false,
        }],
        locals: vec![Locals::default()],
        common: post
            .variables
            .common
            .iter()
            .filter_map(|(n, &v)| Some((n.parse().ok()?, Some(v))))
            .collect(),
        machine,
        result,
        line: 0,
        last_distance: None,
        timers: [0.; 2],
        stopped: false,
        optional_stop,
        through_stops,
        last_label: 0,
    };
    engine.machine.initialize_bounds(&mut engine.result);
    engine.result.report.post_name = post.name.clone();
    engine.result.report.dialect = post.dialect.name.clone();
    if let Some(line) = programs[0].header {
        engine.line = line;
        engine.emit(programs[0].name.clone());
    }
    for _ in 0..1_000_000 {
        if engine.stopped {
            engine.machine.finish(&mut engine.result);
            return engine.result;
        }
        let Some(frame) = engine.frames.last_mut() else {
            engine.machine.finish(&mut engine.result);
            return engine.result;
        };
        let program = &programs[frame.program];
        engine.machine.program_name = if program.name.is_empty() {
            format!("{}{:04}", engine.dialect.program_address, program.number)
        } else {
            program.name.clone()
        };
        let pc = frame.pc;
        let Some(block) = program.blocks.get(pc) else {
            engine.return_frame();
            continue;
        };
        frame.pc += 1;
        engine.line = block.line;
        if block.skip & block_skip != 0 {
            continue;
        }
        let call_stack = trace.as_ref().map(|_| engine.call_stack());
        let var_start = engine.result.var_trace.len();
        let frame = engine.frames.last_mut().unwrap();
        if frame.pending_args {
            frame.pending_args = false;
            let values = engine.locals[frame.locals].values;
            let named = engine.locals[frame.locals].named.clone();
            for (i, value) in values.into_iter().enumerate() {
                if value.is_some() {
                    engine.record(i as u32 + 1, value);
                }
            }
            for (id, value) in named {
                engine.record(id, value);
            }
        }
        match engine.body(&programs, program, pc, &block.body, block.skip) {
            Ok(()) => {}
            Err(mut message) => {
                if engine.stopped {
                    if let Some(comment) = &block.comment {
                        message.push_str(" (");
                        message.push_str(comment);
                        message.push(')');
                    }
                }
                let stack = engine.call_stack();
                let message = if stack.is_empty() {
                    message
                } else {
                    format!("{message} ({})", stack.join(" > "))
                };
                engine.result.diagnostics.push(Diagnostic {
                    src_line: block.line,
                    message,
                });
            }
        }
        if !engine
            .result
            .report
            .programs
            .get(&engine.machine.program_name)
            .is_some_and(|report| report.used_lines.contains(&block.line))
        {
            engine
                .machine
                .account_program_line(&mut engine.result, block.line);
        }
        if let Some(label) = block.label {
            engine.last_label = label;
        }
        if let (Some(trace), Some(stack)) = (trace.as_mut(), call_stack) {
            trace(block.line, &stack, &engine.result.var_trace[var_start..]);
        }
    }
    engine.result.diagnostics.push(Diagnostic {
        src_line: engine.line,
        message: "Execution block limit".into(),
    });
    engine.machine.finish(&mut engine.result);
    engine.result
}

fn integer(value: f64) -> Result<u32> {
    if value < 0. || value.round() > u32::MAX as f64 {
        Err(format!("Invalid integer {value}"))
    } else {
        Ok(value.round() as u32)
    }
}

impl Engine<'_> {
    fn call_stack(&self) -> Vec<String> {
        self.frames
            .iter()
            .filter_map(|frame| frame.site.clone())
            .collect()
    }

    fn emit(&mut self, text: String) {
        self.result.expanded.push(ExpandedLine {
            text,
            src_line: self.line,
            call_stack: self.call_stack(),
            source_g_codes: None,
            modal: {
                let mut snapshot = self.machine.snapshot();
                let source = |canonical| {
                    self.source_modal
                        .values()
                        .rev()
                        .find_map(|&(value, source)| (value == canonical).then_some(source))
                        .unwrap_or_else(|| self.dialect.source_code(b'G', canonical))
                };
                snapshot.work_coordinate = source(snapshot.work_coordinate);
                snapshot.plane = source(snapshot.plane);
                snapshot.g_codes = self
                    .source_modal
                    .iter()
                    .map(|(&g, &(_, v))| (g, v))
                    .collect();
                if let Some((group, code)) = self
                    .dialect
                    .modal_code(self.read(4014).ok().flatten().unwrap_or(67.))
                {
                    snapshot.g_codes.insert(group, code);
                }
                snapshot
            },
        });
    }

    fn record_call(&mut self, kind: CallKind, src_line: usize) {
        if self.through_stops {
            self.result.calls.push(CallEvent {
                src_line,
                kind,
                expanded_index_after: self.result.expanded.len(),
            });
        }
    }

    fn local(&self) -> usize {
        self.frames.last().unwrap().locals
    }

    fn read(&self, n: u32) -> Result<Option<f64>> {
        match n {
            0 => Ok(None),
            1..=33 => Ok(self.locals[self.local()].values[n as usize - 1]),
            100..=199 | 500..=999 => Ok(self.common.get(&n).copied().flatten()),
            OSP_LOCAL_BASE.. => Ok(self.locals[self.local()].named.get(&n).copied().flatten()),
            OSP_COMMON_BASE.. => Ok(self.common.get(&n).copied().flatten()),
            3003 | 3004 => Ok(Some(
                self.common.get(&n).copied().flatten().unwrap_or_default(),
            )),
            3001 => Ok(Some(self.result.stats.time_sec * 1000. - self.timers[0])),
            3002 => Ok(Some(self.result.stats.time_sec / 3600. - self.timers[1])),
            4014 => Ok(Some(if self.frames.last().unwrap().modal.is_some() {
                66.
            } else {
                67.
            })),
            4114 => Ok(Some(self.last_label as f64)),
            4115 => Ok(Some(self.frames.last().unwrap().program_number as f64)),
            _ => self
                .machine
                .read(n)
                .map(Some)
                .ok_or_else(|| format!("Unsupported variable #{n}")),
        }
    }

    fn record(&mut self, n: u32, value: Option<f64>) {
        self.result.var_trace.push(VarChange {
            src_line: self.line,
            var: n,
            value,
        });
    }

    fn write(&mut self, n: u32, value: Option<f64>) -> Result<()> {
        match n {
            1..=33 => {
                let slot = self.local();
                self.locals[slot].values[n as usize - 1] = value;
                self.locals[slot].implicit_decimal[n as usize - 1] = false;
            }
            OSP_LOCAL_BASE.. => {
                let slot = self.local();
                self.locals[slot].named.insert(n, value);
            }
            OSP_COMMON_BASE.. | 100..=199 | 500..=999 | 3003 | 3004 => {
                self.common.insert(n, value);
            }
            3000 | 3006 => {
                self.stopped = true;
                self.record(n, value);
                let v = value.unwrap_or_default();
                return Err(if n == 3000 {
                    format!("Macro alarm {v}")
                } else {
                    format!("Macro stop {v}")
                });
            }
            3001 => self.timers[0] = self.result.stats.time_sec * 1000. - value.unwrap_or_default(),
            3002 => self.timers[1] = self.result.stats.time_sec / 3600. - value.unwrap_or_default(),
            _ if self.machine.write(n, value) => {}
            _ => return Err(format!("Cannot write variable #{n}")),
        }
        self.record(n, value);
        Ok(())
    }

    fn eval(&self, expr: &Expr) -> Result<Option<f64>> {
        let n = |expr| self.eval(expr).map(Option::unwrap_or_default);
        let v = match expr {
            Expr::Number(v) => *v,
            Expr::Var(index) => return self.read(integer(n(index)?)?),
            Expr::Negative(a) => -n(a)?,
            Expr::Binary(op, a, b) => {
                let a = self.eval(a)?;
                let b = self.eval(b)?;
                let x = a.unwrap_or_default();
                let y = b.unwrap_or_default();
                let equal = a.is_some() == b.is_some() && (x - y).abs() <= 1e-9;
                let truth = |v| if v { 1. } else { 0. };
                match op {
                    Op::Add => x + y,
                    Op::Sub => x - y,
                    Op::Mul => x * y,
                    Op::Div => x / y,
                    Op::Mod => x % y,
                    Op::Eq => truth(equal),
                    Op::Ne => truth(!equal),
                    Op::Lt => truth(x < y),
                    Op::Le => truth(x <= y),
                    Op::Gt => truth(x > y),
                    Op::Ge => truth(x >= y),
                    Op::And => ((x as i64) & (y as i64)) as f64,
                    Op::Or => ((x as i64) | (y as i64)) as f64,
                    Op::Xor => ((x as i64) ^ (y as i64)) as f64,
                }
            }
            Expr::Function(name, a, b) => {
                let x = n(a)?;
                match name.as_str() {
                    "SIN" => x.to_radians().sin(),
                    "COS" => x.to_radians().cos(),
                    "TAN" => x.to_radians().tan(),
                    "ASIN" => x.asin().to_degrees(),
                    "ACOS" => x.acos().to_degrees(),
                    "ATAN" => {
                        if let Some(b) = b {
                            x.atan2(n(b)?).to_degrees()
                        } else {
                            x.atan().to_degrees()
                        }
                    }
                    "SQRT" => x.sqrt(),
                    "ABS" => x.abs(),
                    "ROUND" => x.round(),
                    "FIX" => x.trunc(),
                    "FUP" => x.abs().ceil().copysign(x),
                    "LN" => x.ln(),
                    "EXP" => x.exp(),
                    "BIN" => digits(x, 16, 10)?,
                    "BCD" => digits(x, 10, 16)?,
                    "ADP" => {
                        let implicit = if let Expr::Var(index) = a.as_ref() {
                            let id = integer(n(index)?)?;
                            (1..=26).contains(&id)
                                && self.locals[self.local()].implicit_decimal[id as usize - 1]
                        } else {
                            false
                        };
                        if implicit {
                            x / self.post.codes.input_scale()
                        } else {
                            x
                        }
                    }
                    _ => unreachable!(),
                }
            }
        };
        if v.is_finite() {
            Ok(Some(v))
        } else {
            Err("Non-finite expression result".into())
        }
    }

    fn body(
        &mut self,
        programs: &[Program],
        program: &Program,
        pc: usize,
        body: &Body,
        skip: u16,
    ) -> Result<()> {
        let n = |expr| self.eval(expr).map(Option::unwrap_or_default);
        match body {
            Body::Assign(index, value) => self.write(integer(n(index)?)?, self.eval(value)?)?,
            Body::If(cond, branch) => {
                if n(cond)? != 0. {
                    self.body(programs, program, pc, branch, skip)?;
                }
            }
            Body::Goto(target) => {
                let target = integer(n(target)?)?;
                self.frames.last_mut().unwrap().pc = *program
                    .labels
                    .get(&target)
                    .ok_or_else(|| format!("Missing N{target} in O{:04}", program.number))?;
            }
            Body::While(cond, _) => {
                if n(cond)? == 0. {
                    self.frames.last_mut().unwrap().pc =
                        program.loops.get(&pc).ok_or("Missing END")? + 1;
                }
            }
            Body::End(_) => {
                self.frames.last_mut().unwrap().pc = *program.loops.get(&pc).ok_or("Missing DO")?;
            }
            Body::OspCall {
                target,
                name,
                modal,
                args,
            } => {
                if !programs
                    .iter()
                    .any(|p| p.header.is_some() && p.number == *target)
                {
                    return Err(format!("Missing program {name}"));
                }
                let mut locals = Locals::default();
                for (id, expr) in args {
                    let value = self.eval(expr)?;
                    locals.named.insert(*id, value);
                }
                let call = Call {
                    program: *target,
                    repeats: 1,
                    locals: Some(locals),
                };
                if *modal {
                    self.frames.last_mut().unwrap().modal = Some(call);
                } else {
                    self.call(programs, program, call, false)?;
                }
            }
            Body::OspReturn => self.return_frame(),
            Body::OspModalCancel => self.frames.last_mut().unwrap().modal = None,
            Body::Words(words) if !words.is_empty() => {
                self.words(programs, program, words, skip)?
            }
            Body::Words(_) => {}
        }
        Ok(())
    }

    fn words(
        &mut self,
        programs: &[Program],
        program: &Program,
        raw: &[Word],
        skip: u16,
    ) -> Result<()> {
        self.source_words.clear();
        let mut evaluated = Vec::with_capacity(raw.len());
        if let Some((target, locals)) = self.normalize_words(raw, &mut evaluated, 0)? {
            return self.call(
                programs,
                program,
                Call {
                    program: target,
                    repeats: 1,
                    locals,
                },
                false,
            );
        }
        let retained = retained_words(
            &evaluated,
            &self.source_words,
            &self.post.codes,
            self.line,
            &mut self.result.warnings,
        )?;
        let mut flags = retained.iter();
        evaluated.retain(|_| *flags.next().unwrap());
        let mut flags = retained.iter();
        self.source_words.retain(|_| *flags.next().unwrap());
        let mut words: Vec<_> = evaluated.iter().map(|&(a, v, _, _)| (a, v)).collect();
        let codes = &self.post.codes;
        let has = |addr, value| words.contains(&(addr, value));
        let modal = has(b'G', 66.);
        let macro_call = modal || has(b'G', 65.);
        if macro_call || has(b'M', 98.) {
            let p = integer(word(&words, b'P').ok_or("Missing call P")?)?;
            let packed = !macro_call && p > 9999 && !programs.iter().any(|other| other.number == p);
            let target = if packed { p % 10000 } else { p };
            let repeats = integer(word(&words, b'L').unwrap_or(if packed {
                (p / 10000) as f64
            } else {
                1.
            }))? as usize;
            let locals = if macro_call {
                Some(self.arguments(raw)?)
            } else {
                None
            };
            let call = Call {
                program: target,
                repeats,
                locals,
            };
            if modal {
                if !programs
                    .iter()
                    .any(|p| p.header.is_some() && p.number == target)
                {
                    return Err(format!("Missing program O{target:04}"));
                }
                self.frames.last_mut().unwrap().modal = Some(Call {
                    repeats: repeats.max(1),
                    ..call
                });
            } else {
                let depth = self.frames.len();
                self.call(programs, program, call, false)?;
                if !macro_call && self.frames.len() > depth {
                    self.record_call(CallKind::M98, self.line);
                }
            }
            return Ok(());
        }
        if has(b'G', 67.) {
            self.frames.last_mut().unwrap().modal = None;
        }
        if has(b'M', 99.) {
            let source = self.line;
            if self.frames.len() == 1 {
                if let Some(p) = word(&words, b'P') {
                    let n = integer(p)?;
                    self.frames.last_mut().unwrap().pc = *program
                        .labels
                        .get(&n)
                        .ok_or_else(|| format!("Missing N{n}"))?;
                } else {
                    self.return_frame();
                }
            } else {
                self.return_frame();
                if let Some(p) = word(&words, b'P') {
                    let frame = self.frames.last_mut().unwrap();
                    let n = integer(p)?;
                    frame.pc = *programs[frame.program]
                        .labels
                        .get(&n)
                        .ok_or_else(|| format!("Missing N{n}"))?;
                }
            }
            self.record_call(CallKind::M99, source);
            return Ok(());
        }
        // G codes pass local macro arguments. Other special codes call subprograms.
        for &(addr, value) in &words {
            let (target, register) = match addr {
                b'G' => (codes.g_calls.get(&value.to_string()).copied(), None),
                b'M' => (codes.m_calls.get(&value.to_string()).copied(), None),
                b'T' if codes.t_call => (Some(9000), Some(149)),
                b'S' if codes.s_call => (Some(9029), Some(147)),
                b'B' if codes.b_call => (Some(9028), Some(146)),
                _ => continue,
            };
            let Some(target) = target else { continue };
            if self
                .frames
                .iter()
                .any(|frame| frame.program_number == target)
            {
                continue;
            }
            if let Some(n) = register {
                self.write(n, Some(value))?;
            }
            let locals = if addr == b'G' {
                Some(self.arguments(raw)?)
            } else {
                None
            };
            return self.call(
                programs,
                program,
                Call {
                    program: target,
                    repeats: 1,
                    locals,
                },
                false,
            );
        }
        let stop = if has(b'M', 0.) {
            Some(StopKind::M00)
        } else if has(b'M', 1.) {
            Some(StopKind::M01)
        } else {
            None
        };
        let ends = has(b'M', 30.)
            || has(b'M', 2.)
            || (!self.through_stops
                && (stop == Some(StopKind::M00) || (self.optional_stop && stop.is_some())));
        let dwell = has(b'G', 4.) || has(b'G', f64::from(DWELL_FEED));
        let g10 = has(b'G', 10.);
        let length_update = has(b'G', f64::from(DYNAMIC_LENGTH));
        let reference = has(b'G', 28.) || has(b'G', 30.);
        let machine_coord = has(b'G', 53.);
        for (&(_, _, _, group), &((_, canonical), (_, original))) in
            evaluated.iter().zip(&self.source_words)
        {
            if let Some(group) = group.filter(|_| ![65., 66., 67.].contains(&canonical)) {
                self.source_modal.insert(group, (canonical, original));
            }
        }
        words.clear();
        for (address, mut value, implicit, _) in evaluated {
            if implicit && b"XYZIJKRQUVW".contains(&address) && !dwell {
                value *= codes.input_scale();
            }
            if (address, value) != (b'G', 67.) {
                words.push((address, value));
            }
        }
        let path_start = self.result.path.len();
        let changes_start = self.result.var_trace.len();
        let start = self.machine.machine_position();
        let moved = self.machine.block(&mut words, self.line, &mut self.result);
        self.machine.account_block(&mut self.result, self.line);
        if self.result.path[path_start..]
            .iter()
            .any(|segment| segment.kind == SegKind::Cut && segment.time_sec == 0.)
        {
            self.result.report.warnings += 1;
        }
        let moved = moved?;
        let expand = &self.post.expand;
        let motion = self.machine.modal(4001) as u32;
        let distance = self.machine.modal(4003);
        let desired = match expand.distance_mode {
            DistanceMode::Preserve => distance,
            DistanceMode::Absolute => 90.,
            DistanceMode::Incremental => 91.,
        };
        if !g10
            && !length_update
            && !dwell
            && !reference
            && expand.cycles
            && (motion == 73 || (81..=89).contains(&motion))
        {
            words.retain(|&(a, v)| {
                !b"XYZIJKRQLP".contains(&a)
                    && !(a == b'G'
                        && (v == 73.
                            || (81. ..=89.).contains(&v)
                            || [90., 91., 98., 99.].contains(&v)))
            });
            let index = self.result.expanded.len();
            self.emit_words(&mut words, skip);
            if let Some(line) = self.result.expanded.get_mut(index) {
                line.modal.position = self.machine.with_rotary(self.machine.work_at(start));
            }
            for i in path_start..self.result.path.len() {
                let seg = &self.result.path[i];
                let from = seg.from;
                let to = seg.to;
                let point = self.machine.output_position(to);
                let mut line = vec![
                    (b'G', desired),
                    (b'G', if seg.kind == SegKind::Rapid { 0. } else { 1. }),
                ];
                for (i, a) in b"XYZ".iter().copied().enumerate() {
                    if from[i] != to[i] {
                        line.push((
                            a,
                            if desired == 91. {
                                to[i] - from[i]
                            } else {
                                point[i]
                            },
                        ));
                    }
                }
                self.result.path[i].expanded_index = self.result.expanded.len();
                let motion_code = line[1].1;
                let index = self.result.expanded.len();
                self.emit_words(&mut line, skip);
                if let Some(line) = self.result.expanded.get_mut(index) {
                    line.modal.position = self.machine.with_rotary(self.machine.work_at(to));
                    line.modal.absolute = desired != 91.;
                    for code in [motion_code, desired] {
                        if let Some((group, generated)) = self.dialect.modal_code(code) {
                            if let Some(&(_, source)) = self.source_modal.get(&group) {
                                if self.through_stops && source != generated {
                                    line.source_g_codes
                                        .get_or_insert_with(Default::default)
                                        .insert(group, source);
                                }
                            }
                            line.modal.g_codes.insert(group, generated);
                        }
                    }
                }
            }
        } else {
            if g10 && expand.g10 && distance == 91. {
                let changes = &self.result.var_trace[changes_start..];
                for (addr, value) in &mut words {
                    let change = match *addr {
                        b'X' | b'Y' | b'Z' => changes.iter().find(|c| {
                            c.var >= 5201 && (c.var - 5201) % 20 == u32::from(*addr - b'X')
                        }),
                        b'R' => changes.first(),
                        _ => None,
                    };
                    if let Some(v) = change.and_then(|c| c.value) {
                        *value = v;
                    }
                }
                words.retain(|w| *w != (b'G', 91.));
                words.insert(0, (b'G', 90.));
                self.emit_words(&mut words, skip);
                self.emit_words(&mut vec![(b'G', 91.)], skip);
            } else {
                if moved && motion <= 3 && !reference {
                    let end = self.machine.machine_position();
                    let from = self.machine.output_position(start);
                    let to = self.machine.output_position(end);
                    let convert = !machine_coord && desired != distance;
                    words.retain_mut(|(a, v)| {
                        if let Some(i) = b"XYZ".iter().position(|axis| axis == a) {
                            // Arc endpoint axes retain their plane and return semantics.
                            if expand.omit_zero_moves && start[i] == end[i] && motion < 2 {
                                return false;
                            }
                            if convert {
                                *v = if desired == 91. {
                                    to[i] - from[i]
                                } else {
                                    to[i]
                                };
                            }
                        }
                        !(convert && *a == b'G' && [90., 91.].contains(v))
                    });
                    if convert {
                        words.insert(0, (b'G', desired));
                        for (i, a) in b"XYZ".iter().copied().enumerate() {
                            if start[i] != end[i] && word(&words, a).is_none() {
                                words.push((
                                    a,
                                    if desired == 91. {
                                        to[i] - from[i]
                                    } else {
                                        to[i]
                                    },
                                ));
                            }
                        }
                    }
                }
                let unconverted = g10
                    || length_update
                    || reference
                    || (motion == 73 || (81..=89).contains(&motion)) && !dwell;
                if unconverted
                    && self.last_distance.unwrap_or(90.) != distance
                    && !words
                        .iter()
                        .any(|&(a, v)| a == b'G' && [90., 91.].contains(&v))
                {
                    words.insert(0, (b'G', distance));
                } else if !unconverted && !machine_coord {
                    for (a, v) in &mut words {
                        if *a == b'G' && [90., 91.].contains(v) {
                            *v = desired;
                        }
                    }
                    if moved
                        && self.last_distance.unwrap_or(90.) != desired
                        && !words
                            .iter()
                            .any(|&(a, v)| a == b'G' && [90., 91.].contains(&v))
                    {
                        words.insert(0, (b'G', desired));
                    }
                }
                self.emit_words(&mut words, skip);
            }
        }
        if let Some(kind) = stop.filter(|_| self.through_stops) {
            self.result.stops.push(StopPoint {
                expanded_index: self.result.expanded.len() - 1,
                src_line: self.line,
                kind,
            });
        }
        if ends {
            self.stopped = true;
        } else if moved && !self.frames.last().unwrap().in_modal_call {
            if let Some(call) = self.frames.last().unwrap().modal.clone() {
                self.call(programs, program, call, true)?;
            }
        }
        Ok(())
    }

    fn normalize_words(
        &mut self,
        raw: &[Word],
        output: &mut Vec<EvaluatedWord>,
        depth: usize,
    ) -> Result<Option<(u32, Option<Locals>)>> {
        if depth > 8 {
            return Err("Custom replacement nesting limit".into());
        }
        for w in raw {
            let Some(value) = self.eval(&w.value)? else {
                continue;
            };
            let address = self.dialect.address(w.addr);
            let codes = &self.post.codes;
            if (address == b'G' && codes.ignore_g.contains(&value))
                || (address == b'M' && codes.ignore_m.iter().any(|&m| f64::from(m) == value))
            {
                continue;
            }
            let (action, group) = self.dialect.action(w.addr, value);
            match action {
                Action::Ignore => {}
                Action::Unsupported => {
                    return Err(format!(
                        "Unsupported {}{value} in {}",
                        w.addr as char, self.post.dialect.name
                    ))
                }
                Action::Word(a, v) => {
                    self.source_words.push(((a, v), (w.addr, value)));
                    output.push((a, v, w.literal_point == Some(false), group));
                }
                Action::Custom(crate::CustomCode::Call(target)) => {
                    let locals = (address == b'G').then(|| self.arguments(raw)).transpose()?;
                    return Ok(Some((target, locals)));
                }
                Action::Custom(crate::CustomCode::Replace(text)) => {
                    let words = syntax::parse_replacement(&text)?;
                    if let Some(call) = self.normalize_words(&words, output, depth + 1)? {
                        return Ok(Some(call));
                    }
                }
            }
        }
        Ok(None)
    }

    fn arguments(&mut self, raw: &[Word]) -> Result<Locals> {
        let mut locals = Locals::default();
        let mut seen = [false; 33];
        let mut duplicates = [None; 33];
        let mut last_ijk = None;
        let mut ijk_group = 0;
        for w in raw {
            let addr = self.dialect.address(w.addr);
            let Some(mut slot) = argument(addr) else {
                continue;
            };
            if matches!(addr, b'I' | b'J' | b'K') {
                if last_ijk.is_some_and(|last| addr <= last) {
                    ijk_group += 1;
                }
                last_ijk = Some(addr);
                if ijk_group == 10 {
                    return Err("Too many I/J/K macro argument groups (maximum 10)".into());
                }
                slot += 3 * ijk_group;
            }
            let implicit = w.literal_point == Some(false)
                && !b"FHMST".contains(&addr)
                && self.post.codes.integer_units == IntegerUnits::LeastIncrement;
            let value = self.eval(&w.value)?.map(|v| {
                if implicit {
                    v * self.post.codes.input_scale()
                } else {
                    v
                }
            });
            if seen[slot] {
                let label = format!("macro argument {}", addr as char);
                let old = locals.values[slot];
                let policy = self.post.codes.duplicate_word;
                if policy == crate::DuplicateWord::Error {
                    let show =
                        |v: Option<f64>| v.map_or_else(|| "vacant".into(), |v| v.to_string());
                    return Err(format!(
                        "Duplicate {label}: {}, {}; block rejected",
                        show(old),
                        show(value)
                    ));
                }
                duplicates[slot] = Some(addr);
                if policy == crate::DuplicateWord::First {
                    continue;
                }
            }
            seen[slot] = true;
            locals.values[slot] = value;
            locals.implicit_decimal[slot] = implicit;
        }
        if self.post.codes.warn_on_duplicate {
            for (address, value) in duplicates.into_iter().zip(locals.values) {
                if let Some(address) = address {
                    let adopted = value.map_or_else(|| "vacant".into(), |v| v.to_string());
                    self.result.warnings.push(Diagnostic {
                        src_line: self.line,
                        message: format!(
                            "Duplicate macro argument {}: kept {adopted}",
                            address as char
                        ),
                    });
                }
            }
        }
        Ok(locals)
    }

    fn emit_words(&mut self, words: &mut Vec<(u8, f64)>, skip: u16) {
        self.format_block(words);
        if !words.is_empty() {
            let dwell =
                words.contains(&(b'G', 4.)) || words.contains(&(b'G', f64::from(DWELL_FEED)));
            let text = words
                .iter()
                .map(|&(a, v)| {
                    let original = self
                        .source_words
                        .iter()
                        .rev()
                        .find(|&&((address, value), _)| {
                            address == a && (!b"GM".contains(&a) || value == v)
                        })
                        .map(|&(_, original)| original);
                    let (letter, value) = original.map_or_else(
                        || {
                            (
                                self.dialect.source_address(a),
                                self.dialect.source_code(a, v),
                            )
                        },
                        |(letter, value)| (letter, if b"GM".contains(&a) { value } else { v }),
                    );
                    format!("{}{}", letter as char, display(a, value, self.post, dwell))
                })
                .collect::<String>();
            let text = match skip {
                0 => text,
                1 => format!("/{text}"),
                _ => {
                    let mut prefix = String::new();
                    for level in 1..=9 {
                        if skip & (1 << (level - 1)) != 0 {
                            prefix.push('/');
                            prefix.push(char::from(b'0' + level));
                        }
                    }
                    prefix.push_str(&text);
                    prefix
                }
            };
            self.emit(text);
            if let Some(distance) = self.last_distance {
                let line = self.result.expanded.last_mut().unwrap();
                let snapshot = &mut line.modal;
                snapshot.absolute = distance != 91.;
                if let Some((group, source)) = self.dialect.modal_code(distance) {
                    if let Some(&(_, original)) = self
                        .source_modal
                        .get(&group)
                        .filter(|&&(v, _)| v == distance)
                    {
                        snapshot.g_codes.insert(group, original);
                    } else {
                        if self.through_stops {
                            if let Some(&(_, original)) = self.source_modal.get(&group) {
                                if original != source {
                                    line.source_g_codes
                                        .get_or_insert_with(Default::default)
                                        .insert(group, original);
                                }
                            }
                        }
                        snapshot.g_codes.insert(group, source);
                    }
                }
            }
        }
    }

    fn format_block(&mut self, words: &mut Vec<(u8, f64)>) {
        let mixed = words.len() > 1;
        words.retain(|&(a, v)| {
            if a != b'G' || ![90., 91.].contains(&v) {
                return true;
            }
            let repeated = self.last_distance.replace(v) == Some(v);
            !(mixed && repeated)
        });
        if let Some(p) = words.iter().position(|w| w.0 == b'P') {
            if let Some(g) = words
                .iter()
                .position(|&(a, v)| a == b'G' && [82., 88., 89.].contains(&v))
            {
                if p > g + 1 {
                    words[g + 1..=p].rotate_right(1);
                }
            }
        }
    }

    fn call(
        &mut self,
        programs: &[Program],
        caller: &Program,
        call: Call,
        modal: bool,
    ) -> Result<()> {
        if call.repeats == 0 {
            return Ok(());
        }
        if self.frames.len() >= 16 {
            return Err("Call nesting limit".into());
        }
        let index = programs
            .iter()
            .position(|p| p.header.is_some() && p.number == call.program)
            .ok_or_else(|| format!("Missing program O{:04}", call.program))?;
        let parent = self.frames.last().unwrap();
        let in_modal_call = modal || parent.in_modal_call;
        let inherited = parent.modal.clone();
        let mut locals = parent.locals;
        let owns_locals = call.locals.is_some();
        if let Some(args) = call.locals {
            if self.locals.len() >= 5 {
                return Err("Macro nesting limit".into());
            }
            locals = self.locals.len();
            self.locals.push(args);
        }
        self.frames.push(Frame {
            program: index,
            program_number: call.program,
            pc: 0,
            repeat: call.repeats,
            locals,
            owns_locals,
            site: Some(format!(
                "{}:{}",
                if caller.name.is_empty() {
                    "O0000"
                } else {
                    &caller.name
                },
                self.line
            )),
            modal: inherited,
            in_modal_call,
            pending_args: owns_locals,
        });
        Ok(())
    }

    fn return_frame(&mut self) {
        let frame = self.frames.last_mut().unwrap();
        if frame.repeat > 1 {
            frame.repeat -= 1;
            frame.pc = 0;
        } else if self.frames.pop().unwrap().owns_locals {
            self.locals.pop();
        }
    }
}

fn argument(addr: u8) -> Option<usize> {
    b"ABCIJKDEFH MQRSTUVWXYZ"
        .iter()
        .filter(|&&c| c != b' ')
        .position(|&c| c == addr)
        .map(|i| {
            [
                0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
            ][i]
        })
}

fn digits(x: f64, from: u64, to: u64) -> Result<f64> {
    let mut n = u64::from(integer(x.abs())?);
    let mut output = 0u64;
    let mut place = 1u64;
    while n != 0 {
        let digit = n % from;
        if digit > 9 {
            return Err("Invalid BCD digit".into());
        }
        output += digit * place;
        place *= to;
        n /= from;
    }
    Ok((output as f64).copysign(x))
}

fn display(addr: u8, v: f64, post: &PostProfile, dwell: bool) -> String {
    let places = usize::from(post.codes.decimal_places);
    let dimension = b"XYZIJKRQUVW".contains(&addr) && !(dwell && addr == b'U');
    if dimension && !dwell && !post.expand.decimal_point {
        return format!("{:.0}", (v / post.codes.input_scale()).round() + 0.);
    }
    let mut s = format!("{v:.places$}");
    if places != 0 {
        s.truncate(s.trim_end_matches('0').trim_end_matches('.').len());
    }
    if s == "-0" {
        s.remove(0);
    }
    if dimension && post.expand.decimal_point && !s.contains('.') {
        s.push('.');
    }
    s
}
