//! 機械座標系の工具中心経路。各補正ブロックは次の移動と接続する。

use std::{f64::consts::TAU, ops::Range};

use crate::{Diagnostic, MotionTotals, RunResult, SegKind};

const EPS: f64 = 1e-8;
const LOOKAHEAD: usize = 3;
type Point = [f64; 2];

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Mode {
    Off,
    Offset(f64),
    Missing,
}

#[derive(Clone, Copy)]
pub(crate) struct Arc {
    pub center: Point,
    pub start: f64,
    /// 反時計回りを正とするラジアン。
    pub sweep: f64,
    pub radius: f64,
}

#[derive(Clone, Copy)]
struct Curve {
    start: Point,
    end: Point,
    arc: Option<Arc>,
}

pub(crate) struct Block {
    pub line: usize,
    pub range: Range<usize>,
    pub plane: [usize; 3],
    pub mode: Mode,
    pub supported: bool,
    pub program: String,
}

struct Pending {
    block: Block,
    curve: Curve,
    interval: [f64; 2],
    original_length: f64,
    lead_in: bool,
    lead_out: bool,
    serial: usize,
    tool: usize,
}

pub(crate) struct CutterComp {
    applied: Mode,
    pending: Option<Pending>,
    pub arc: Option<Arc>,
    serial: usize,
    measure: bool,
    delta: Vec<Measurement>,
}

struct Measurement {
    program: String,
    tool: usize,
    kind: SegKind,
    distance: f64,
    time: f64,
}

impl CutterComp {
    pub fn new(measure: bool) -> Self {
        Self {
            applied: Mode::Off,
            pending: None,
            arc: None,
            serial: 0,
            measure,
            delta: Vec::new(),
        }
    }

    pub fn record(&mut self, result: &mut RunResult, block: Block) {
        self.serial += 1;
        let arc = self.arc.take();
        if block.range.is_empty() {
            return;
        }
        let previous_mode = std::mem::replace(&mut self.applied, block.mode);
        let radius = match (previous_mode, block.mode) {
            (Mode::Off, Mode::Off) => {
                self.flush(result);
                return;
            }
            (_, Mode::Missing) | (Mode::Missing, Mode::Off) => {
                if previous_mode != Mode::Missing {
                    warn(
                        result,
                        block.line,
                        "G41/G42 requires a D number with a valid offset",
                    );
                }
                self.flush(result);
                return;
            }
            (_, Mode::Offset(r)) | (Mode::Offset(r), Mode::Off) => r,
        };
        let lead_in = !matches!(previous_mode, Mode::Offset(_));
        let lead_out = block.mode == Mode::Off;
        if !block.supported || (arc.is_some() && (lead_in || lead_out)) {
            warn(
                result,
                block.line,
                "Unsupported motion with cutter compensation",
            );
            self.flush(result);
            self.applied = Mode::Off;
            return;
        }
        let segments = &result.path[block.range.clone()];
        let from = segments.first().unwrap().from;
        let to = segments.last().unwrap().to;
        let tool = segments.first().unwrap().tool_index;
        let [u, v, w] = block.plane;
        let start = [from[u], from[v]];
        let end = [to[u], to[v]];
        let planar = distance(start, end);
        let original_length = arc
            .map_or(planar, |a| a.radius * a.sweep.abs())
            .hypot(to[w] - from[w]);
        let curve = if let Some(mut arc) = arc {
            // G41 内側の CCW 円弧では半径が小さくなる。
            arc.radius -= arc.sweep.signum() * radius;
            if arc.radius <= EPS {
                warn(result, block.line, "Cutter compensation collapses the arc");
                self.flush(result);
                self.applied = Mode::Off;
                return;
            }
            Curve {
                start: circle_point(arc, arc.start),
                end: circle_point(arc, arc.start + arc.sweep),
                arc: Some(arc),
            }
        } else if planar > EPS {
            if (lead_in || lead_out) && planar < radius.abs() {
                warn(
                    result,
                    block.line,
                    "Cutter compensation lead move is shorter than the radius",
                );
            }
            let delta = sub(end, start);
            let normal = scale([-delta[1], delta[0]], radius / planar);
            Curve {
                start: add(start, normal),
                end: add(end, normal),
                arc: None,
            }
        } else if !lead_in && !lead_out {
            let Some(last) = self.pending.as_ref().filter(|p| {
                p.block.plane == block.plane && p.block.mode == block.mode && p.tool == tool
            }) else {
                warn(
                    result,
                    block.line,
                    "Cutter compensation has no vector for this axial move",
                );
                return;
            };
            let offset = sub(last.curve.point(last.interval[1]), start);
            Curve {
                start: add(start, offset),
                end: add(end, offset),
                arc: None,
            }
        } else {
            warn(
                result,
                block.line,
                "G41/G42 startup and G40 cancellation require a planar move",
            );
            self.flush(result);
            self.applied = previous_mode;
            return;
        };
        // A startup or cancellation curve keeps the feed move's planar direction
        // for intersection. The unshifted endpoint is applied when writing it.
        let mut current = Pending {
            block,
            curve,
            interval: [0., 1.],
            original_length,
            lead_in,
            lead_out,
            serial: self.serial,
            tool,
        };
        if let Some(mut last) = self.pending.take() {
            if !last.lead_out && !current.lead_in {
                let compatible = last.block.plane == current.block.plane
                    && (last.block.mode == current.block.mode || lead_out)
                    && last.tool == current.tool
                    && current.serial - last.serial <= LOOKAHEAD;
                if compatible && lead_out {
                    current.curve.start = last.curve.point(last.interval[1]);
                } else if compatible {
                    if let Some([a, b]) = join(&last, &current, start, radius.abs()) {
                        last.interval[1] = a;
                        current.interval[0] = b;
                    } else {
                        warn(
                            result,
                            current.block.line,
                            "No valid cutter compensation corner",
                        );
                    }
                } else {
                    warn(
                        result,
                        current.block.line,
                        "Cutter compensation lookahead, plane, tool or D changed",
                    );
                }
            }
            self.write(result, last);
        }
        self.pending = Some(current);
    }

    fn flush(&mut self, result: &mut RunResult) {
        if let Some(last) = self.pending.take() {
            self.write(result, last);
        }
    }

    fn write(&mut self, result: &mut RunResult, pending: Pending) {
        let segments = &mut result.path[pending.block.range];
        let [u, v, w] = pending.block.plane;
        let from = segments.first().unwrap().from;
        let to = segments.last().unwrap().to;
        let start = if pending.lead_in {
            [from[u], from[v]]
        } else {
            pending.curve.point(pending.interval[0])
        };
        let end = if pending.lead_out {
            [to[u], to[v]]
        } else {
            pending.curve.point(pending.interval[1])
        };
        let planar = pending.curve.arc.map_or_else(
            || distance(start, end),
            |arc| arc.radius * arc.sweep.abs() * (pending.interval[1] - pending.interval[0]),
        );
        let compensated_length = planar.hypot(to[w] - from[w]);
        let factor = if pending.original_length > EPS {
            compensated_length / pending.original_length
        } else {
            1.
        };
        let count = segments.len();
        let mut previous = from;
        let mut time = 0.;
        for (i, segment) in segments.iter_mut().enumerate() {
            let fraction = if i + 1 == count {
                1.
            } else if let Some(arc) = pending.curve.arc {
                let [x, y] = sub([segment.to[u], segment.to[v]], arc.center);
                ((y.atan2(x) - arc.start) * arc.sweep.signum()).rem_euclid(TAU) / arc.sweep.abs()
            } else {
                (i + 1) as f64 / count as f64
            };
            let point = if pending.curve.arc.is_some() && i + 1 < count {
                pending.curve.point(
                    pending.interval[0] + fraction * (pending.interval[1] - pending.interval[0]),
                )
            } else {
                end
            };
            let mut next = segment.to;
            next[u] = point[0];
            next[v] = point[1];
            next[w] = from[w] + fraction * (to[w] - from[w]);
            if i == 0 {
                previous[u] = start[0];
                previous[v] = start[1];
            }
            segment.comp_from = Some(previous);
            segment.comp_to = Some(next);
            previous = next;
            if self.measure {
                let compensated_time = segment.time_sec * factor;
                time += compensated_time - segment.time_sec;
                segment.time_sec = compensated_time;
            }
        }
        if self.measure {
            self.delta.push(Measurement {
                program: pending.block.program,
                tool: pending.tool,
                kind: segments[0].kind,
                distance: compensated_length - pending.original_length,
                time,
            });
        }
    }

    pub fn finish(&mut self, result: &mut RunResult) {
        self.flush(result);
        for delta in self.delta.drain(..) {
            match delta.kind {
                SegKind::Rapid => result.stats.rapid_length_mm += delta.distance,
                SegKind::Cut => result.stats.cut_length_mm += delta.distance,
            }
            result.stats.time_sec += delta.time;
            let tool = result.tools[delta.tool].number;
            add_measurement(&mut result.report.totals, &delta);
            if let Some(report) = result.report.tools.get_mut(&tool) {
                add_measurement(&mut report.totals, &delta);
            }
            if let Some(report) = result.report.programs.get_mut(&delta.program) {
                add_measurement(&mut report.totals, &delta);
            }
        }
    }
}

fn add_measurement(totals: &mut MotionTotals, delta: &Measurement) {
    match delta.kind {
        SegKind::Rapid => {
            totals.rapid_length_mm += delta.distance;
            totals.rapid_time_sec += delta.time;
        }
        SegKind::Cut => {
            totals.cut_length_mm += delta.distance;
            totals.cut_time_sec += delta.time;
        }
    }
}

fn warn(result: &mut RunResult, src_line: usize, message: &str) {
    result.warnings.push(Diagnostic {
        src_line,
        message: message.into(),
    });
}

impl Curve {
    fn point(self, t: f64) -> Point {
        self.arc.map_or_else(
            || add(self.start, scale(sub(self.end, self.start), t)),
            |a| circle_point(a, a.start + a.sweep * t),
        )
    }

    fn parameter(self, point: Point, at_end: bool) -> f64 {
        if let Some(arc) = self.arc {
            let [x, y] = sub(point, arc.center);
            let angle = y.atan2(x);
            let endpoint = arc.start + if at_end { arc.sweep } else { 0. };
            let relative = (angle - endpoint + TAU / 2.).rem_euclid(TAU) - TAU / 2.;
            return f64::from(at_end) + relative / arc.sweep;
        }
        let d = sub(self.end, self.start);
        dot(sub(point, self.start), d) / dot(d, d)
    }
}

fn join(previous: &Pending, next: &Pending, corner: Point, radius: f64) -> Option<[f64; 2]> {
    let a = previous.curve;
    let b = next.curve;
    if distance(a.point(previous.interval[1]), b.start) <= EPS {
        return Some([previous.interval[1], 0.]);
    }
    intersections(a, b)
        .into_iter()
        .flatten()
        .filter_map(|point| {
            let a = a.parameter(point, true);
            let b = b.parameter(point, false);
            // Reject reverse travel, excessive extension and joins on a distant loop.
            let gap = distance(point, corner);
            (a > previous.interval[0] + EPS && b < 1. - EPS && gap <= radius * 10. + EPS)
                .then_some((gap, [a, b]))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, pair)| pair)
}

fn intersections(a: Curve, b: Curve) -> [Option<Point>; 2] {
    match (a.arc, b.arc) {
        (None, None) => {
            let d = sub(a.end, a.start);
            let e = sub(b.end, b.start);
            let det = cross(d, e);
            if det.abs() <= EPS {
                [None; 2]
            } else {
                [
                    Some(add(
                        a.start,
                        scale(d, cross(sub(b.start, a.start), e) / det),
                    )),
                    None,
                ]
            }
        }
        (None, Some(circle)) => line_circle(a, circle),
        (Some(circle), None) => line_circle(b, circle),
        (Some(a), Some(b)) => {
            let d = sub(b.center, a.center);
            let span = d[0].hypot(d[1]);
            if span <= EPS {
                return [None; 2];
            }
            let axis = scale(d, 1. / span);
            let x = (a.radius.powi(2) - b.radius.powi(2) + span.powi(2)) / (2. * span);
            let h2 = a.radius.powi(2) - x.powi(2);
            pair(add(a.center, scale(axis, x)), [-axis[1], axis[0]], h2)
        }
    }
}

fn line_circle(line: Curve, circle: Arc) -> [Option<Point>; 2] {
    let d = sub(line.end, line.start);
    let length = d[0].hypot(d[1]);
    if length <= EPS {
        return [None; 2];
    }
    let unit = scale(d, 1. / length);
    let foot = add(
        line.start,
        scale(unit, dot(sub(circle.center, line.start), unit)),
    );
    let h2 = circle.radius.powi(2) - distance(foot, circle.center).powi(2);
    pair(foot, unit, h2)
}

fn pair(mid: Point, unit: Point, h2: f64) -> [Option<Point>; 2] {
    if h2 < -EPS {
        return [None; 2];
    }
    let d = scale(unit, h2.max(0.).sqrt());
    [Some(add(mid, d)), Some(sub(mid, d))]
}

fn circle_point(a: Arc, angle: f64) -> Point {
    let (sin, cos) = angle.sin_cos();
    add(a.center, scale([cos, sin], a.radius))
}

fn add(a: Point, b: Point) -> Point {
    [a[0] + b[0], a[1] + b[1]]
}
fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}
fn scale(a: Point, s: f64) -> Point {
    [a[0] * s, a[1] * s]
}
fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn cross(a: Point, b: Point) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn distance(a: Point, b: Point) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
