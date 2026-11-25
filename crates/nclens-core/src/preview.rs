//! プレビューの再生と元コードの強調に使う実行位置。
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum StopKind {
    M00,
    M01,
}

#[derive(Debug, Clone, Serialize)]
pub struct StopPoint {
    pub expanded_index: usize,
    pub src_line: usize,
    pub kind: StopKind,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum CallKind {
    M98,
    M99,
}

#[derive(Debug, Clone, Serialize)]
pub struct CallEvent {
    pub src_line: usize,
    pub kind: CallKind,
    /// このブロックを処理した時点の展開行数。呼び出し先の先頭・戻る直前の境界。
    pub expanded_index_after: usize,
}
