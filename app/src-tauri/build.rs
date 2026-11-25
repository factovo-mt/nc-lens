//! 同梱のサンプル：repo の samples（_ や . で始まるもの＝手元専用は除く）と app/samples を
//! gen/samples へ写し、tauri.conf.json の resources で exe の横の samples へ入れる。

use std::path::Path;

fn stage(from: &Path, to: &Path) {
    println!("cargo:rerun-if-changed={}", from.display());
    let Ok(entries) = std::fs::read_dir(from) else { return };
    for e in entries.flatten() {
        let name = e.file_name();
        if name.to_string_lossy().starts_with(['_', '.']) {
            continue;
        }
        let (src, dst) = (e.path(), to.join(&name));
        if src.is_dir() {
            stage(&src, &dst);
        } else {
            std::fs::create_dir_all(to).expect("gen/samples を作れません");
            std::fs::copy(&src, &dst).expect("サンプルを写せません");
        }
    }
}

fn main() {
    let out = Path::new("gen/samples");
    let _ = std::fs::remove_dir_all(out);
    stage(Path::new("../../samples"), out);
    stage(Path::new("../samples"), out);
    tauri_build::build()
}
