use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::RunResult;

/// Coordinates in X, Y, Z, A, B, C order, in mm and degrees.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AxisRange {
    pub min: [f64; 6],
    pub max: [f64; 6],
}

impl AxisRange {
    pub(crate) fn include(&mut self, point: [f64; 6]) {
        for (i, value) in point.into_iter().enumerate() {
            self.min[i] = self.min[i].min(value);
            self.max[i] = self.max[i].max(value);
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CoordinateBounds {
    pub work: AxisRange,
    pub machine: AxisRange,
}

impl CoordinateBounds {
    pub(crate) fn at(work: [f64; 6], machine: [f64; 6]) -> Self {
        Self {
            work: AxisRange {
                min: work,
                max: work,
            },
            machine: AxisRange {
                min: machine,
                max: machine,
            },
        }
    }

    pub(crate) fn include(&mut self, work: [f64; 6], machine: [f64; 6]) {
        self.work.include(work);
        self.machine.include(machine);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModalSnapshot {
    pub g_codes: BTreeMap<u32, f64>,
    pub feed: f64,
    pub spindle: f64,
    pub tool: u32,
    pub h: u32,
    pub d: u32,
    pub work_coordinate: f64,
    pub plane: f64,
    pub absolute: bool,
    pub position: [f64; 6],
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MotionTotals {
    pub cut_length_mm: f64,
    pub rapid_length_mm: f64,
    pub other_length_mm: f64,
    pub cut_time_sec: f64,
    pub rapid_time_sec: f64,
    pub other_time_sec: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReportBreakdown {
    pub totals: MotionTotals,
    pub bounds: CoordinateBounds,
    /// Source line numbers, starting at 1.
    pub used_lines: BTreeSet<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub filename: String,
    /// Generation time in seconds since 1970-01-01 UTC.
    pub generated_at_unix_seconds: u64,
    #[serde(rename = "mnemonic")]
    pub dialect: String,
    pub post_name: String,
    pub totals: MotionTotals,
    pub feed_min: Option<f64>,
    pub feed_max: Option<f64>,
    pub bounds: CoordinateBounds,
    /// Keyed by tool number.
    pub tools: BTreeMap<u32, ReportBreakdown>,
    /// Keyed by program name.
    pub programs: BTreeMap<String, ReportBreakdown>,
    pub errors: usize,
    pub warnings: usize,
}

/// Build a serializable report from completed execution.
pub fn make_report(result: &RunResult, filename: &str) -> Report {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    make_report_at(result, filename, timestamp)
}

/// Build a report with a host-provided Unix timestamp in seconds.
pub fn make_report_at(result: &RunResult, filename: &str, timestamp: u64) -> Report {
    let mut report = result.report.clone();
    report.filename = filename.into();
    report.generated_at_unix_seconds = timestamp;
    report.bounds = result.bounds.clone();
    report.errors = result.diagnostics.len();
    report
}
