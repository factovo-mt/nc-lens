//! 同梱のサンプル NC：配布物は exe の横の samples、開発時は repo の samples（手元専用の _local を含む）。

use serde::Serialize;
use std::path::{Path, PathBuf};

/// サンプルとして一覧に出す拡張子
const NC_EXT: [&str; 7] = ["ncd", "nc", "ncl", "lst", "tap", "txt", "min"];

/// サンプルのフォルダ。無ければ None。
pub fn dir() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).map(|p| p.join("samples"));
        if let Some(dev) = repo.filter(|p| p.is_dir()) {
            return Some(dev);
        }
    }
    let exe = std::env::current_exe().ok()?.parent()?.join("samples");
    exe.is_dir().then_some(exe)
}

#[derive(Serialize)]
pub struct Sample {
    /// サンプルのフォルダからの相対パス（区切りは /）
    name: String,
    path: String,
}

/// サンプルのフォルダの NC ファイル（1 段下のフォルダまで）。直下を先に、名前の順。
pub fn list() -> Vec<Sample> {
    let Some(root) = dir() else { return Vec::new() };
    let mut out = Vec::new();
    walk(&root, "", 0, &mut out);
    out.sort_by_key(|s| (s.name.contains('/'), s.name.to_lowercase()));
    out
}

fn walk(dir: &Path, prefix: &str, depth: u32, out: &mut Vec<Sample>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let (path, name) = (e.path(), e.file_name().to_string_lossy().into_owned());
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            if depth == 0 {
                walk(&path, &format!("{prefix}{name}/"), depth + 1, out);
            }
        } else if path.extension().and_then(|x| x.to_str()).is_some_and(|x| NC_EXT.contains(&x.to_ascii_lowercase().as_str())) {
            out.push(Sample { name: format!("{prefix}{name}"), path: path.display().to_string() });
        }
    }
}
