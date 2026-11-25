//! NC Lens のコア：NC の解析・マクロ展開・経路・計測。
//! GUI と CLI が使う公開型と実行の入口。

use serde::Serialize;

/// 展開後の 1 行と、その出どころ。
#[derive(Debug, Clone, Serialize)]
pub struct ExpandedLine {
    pub text: String,
    /// 元ファイルの行番号（1 始まり）
    pub src_line: usize,
    /// 呼び出しの経路（例：["O0010:12", "O0012:9"]）
    pub call_stack: Vec<String>,
    pub modal: ModalSnapshot,
    /// 展開出力で書き換えたモーダル群の、書き換え前の有効コード。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_g_codes: Option<std::collections::BTreeMap<u32, f64>>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum SegKind {
    Rapid,
    Cut,
}

/// 経路の 1 線分（円弧はコアで線分化して渡す）。座標は機械座標系、mm。
#[derive(Debug, Clone, Serialize)]
pub struct Segment {
    pub kind: SegKind,
    pub from: [f64; 3],
    pub to: [f64; 3],
    /// 工具中心の補正経路。補正なし、または補正できないときは None。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comp_from: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comp_to: Option<[f64; 3]>,
    pub src_line: usize,
    /// RunResult.tools の添字（0 始まり）。
    pub tool_index: usize,
    /// この線分の所要時間（秒）。円弧は線分化した長さで按分
    pub time_sec: f64,
    /// この線分を出したブロックの、RunResult.expanded での添字。円弧の線分化や固定サイクルで出た複数の線分は同じ添字
    /// （固定サイクルを展開出力するときは、展開した各行の添字）
    pub expanded_index: usize,
}

/// 変数の変化 1 件。
#[derive(Debug, Clone, Serialize)]
pub struct VarChange {
    pub src_line: usize,
    pub var: u32,
    /// 空（#0 相当）は None
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub src_line: usize,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Stats {
    pub cut_length_mm: f64,
    pub rapid_length_mm: f64,
    pub time_sec: f64,
    pub min: [f64; 3],
    pub max: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockSkipUsage {
    pub level: u8,
    pub line_count: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RunResult {
    pub expanded: Vec<ExpandedLine>,
    /// 元の NC に出現する番号を番号順に列挙する。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub block_skip_usage: Vec<BlockSkipUsage>,
    pub path: Vec<Segment>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stops: Vec<StopPoint>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<CallEvent>,
    pub var_trace: Vec<VarChange>,
    pub diagnostics: Vec<Diagnostic>,
    pub stats: Stats,
    pub bounds: CoordinateBounds,
    pub tool_bounds: std::collections::BTreeMap<u32, CoordinateBounds>,
    pub tools: Vec<tool::ToolInfo>,
    /// 解析・描画・工具認識の警告。NC の展開は継続する。
    pub warnings: Vec<Diagnostic>,
    #[serde(skip)]
    pub(crate) report: Report,
}

mod cutter_comp;
mod dialect;
mod duplicate;
mod engine;
mod machine;
mod macro_settings;
mod post;
mod preview;
mod report;
mod syntax;
mod text_settings;
pub mod tool;

pub use dialect::*;
pub use macro_settings::*;
pub use post::*;
pub use preview::*;
pub use report::*;
pub use text_settings::{
    dialect_from_json, dialect_from_str, dialect_to_json, dialect_to_string, post_from_json,
    post_from_str, post_to_json, post_to_string,
};

/// NC テキストを解析・展開・実行する。
pub fn run(src: &str) -> RunResult {
    run_with(src, &PostProfile::default())
}

/// ポストの設定と初期値で NC を実行する。
pub fn run_with(src: &str, post: &PostProfile) -> RunResult {
    run_with_post_skip_levels(src, post, post.expand.enabled_block_skip_levels(), false)
}

pub fn run_with_post_switches(
    src: &str,
    post: &PostProfile,
    block_skip: bool,
    optional_stop: bool,
) -> RunResult {
    run_with_post_skip_levels(
        src,
        post,
        if block_skip { &[1] } else { &[] },
        optional_stop,
    )
}

/// 有効なスキップ番号と M01 の停止を指定して実行する。
pub fn run_with_skip_levels(src: &str, levels: &[u8], optional_stop: bool) -> RunResult {
    run_with_post_skip_levels(src, &PostProfile::default(), levels, optional_stop)
}

pub fn run_with_post_skip_levels(
    src: &str,
    post: &PostProfile,
    levels: &[u8],
    optional_stop: bool,
) -> RunResult {
    engine::run(src, post, levels, optional_stop, false, None)
}

/// M00・M01 の停止位置を記録し、停止後のブロックも展開する。
pub fn run_through_stops(src: &str, post: &PostProfile) -> RunResult {
    run_through_stops_with_skip_levels(src, post, post.expand.enabled_block_skip_levels())
}

pub fn run_through_stops_with_skip_levels(
    src: &str,
    post: &PostProfile,
    levels: &[u8],
) -> RunResult {
    engine::run(src, post, levels, false, true, None)
}

/// オプショナルブロックスキップと M01 の停止を指定して実行する。
pub fn run_with_switches(src: &str, block_skip: bool, optional_stop: bool) -> RunResult {
    run_with_post_switches(src, &PostProfile::default(), block_skip, optional_stop)
}

/// 実行したブロックの元行、呼び出し経路、変数書き込みを実行順に渡す。
pub fn run_with_trace(
    src: &str,
    block_skip: bool,
    optional_stop: bool,
    trace: impl FnMut(usize, &[String], &[VarChange]),
) -> RunResult {
    run_with_post_trace(
        src,
        &PostProfile::default(),
        block_skip,
        optional_stop,
        trace,
    )
}

pub fn run_with_post_trace(
    src: &str,
    post: &PostProfile,
    block_skip: bool,
    optional_stop: bool,
    trace: impl FnMut(usize, &[String], &[VarChange]),
) -> RunResult {
    run_with_post_trace_and_skip_levels(
        src,
        post,
        if block_skip { &[1] } else { &[] },
        optional_stop,
        trace,
    )
}

pub fn run_with_post_trace_and_skip_levels(
    src: &str,
    post: &PostProfile,
    levels: &[u8],
    optional_stop: bool,
    mut trace: impl FnMut(usize, &[String], &[VarChange]),
) -> RunResult {
    engine::run(src, post, levels, optional_stop, false, Some(&mut trace))
}
