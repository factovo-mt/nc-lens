//! アプリの設定（表示の状態）：`nclens.settings.json`。
//! 置き場所は exe と同じフォルダ。書けなければ %APPDATA%\NC Lens\。
//! 中身の形は画面側（app/src/settings.ts）が決める。ここは読み書きと、起動時のウィンドウの位置・大きさだけ。

use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::{Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow};

const FILE_NAME: &str = "nclens.settings.json";

/// 設定ファイルのパスと、%APPDATA% に置いたか（起動中は同じ場所を使う）。
fn place() -> &'static (PathBuf, bool) {
    static PLACE: OnceLock<(PathBuf, bool)> = OnceLock::new();
    PLACE.get_or_init(|| {
        let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
        match exe_dir {
            Some(dir) if writable(&dir) => (dir.join(FILE_NAME), false),
            _ => (appdata_dir().join(FILE_NAME), true),
        }
    })
}

pub fn path() -> &'static Path {
    &place().0
}

fn appdata_dir() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("NC Lens")
}

/// フォルダに書けるか（Program Files などは書けない）。試しのファイルを作って消す。
fn writable(dir: &Path) -> bool {
    let probe = dir.join(".nclens_write_test");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

#[derive(Serialize)]
pub struct Loaded {
    /// 設定ファイルのパス
    path: String,
    /// exe のフォルダに書けず %APPDATA% に置いた
    fallback: bool,
    /// 読めた設定。ファイルが無い・壊れていれば null（既定で起動）
    value: Option<Value>,
    /// 壊れていた元のファイルを退避した先
    backup: Option<String>,
}

pub fn load_for(startup: Option<&crate::startup::Startup>) -> Loaded {
    match startup.and_then(|args| args.settings.as_ref()) {
        Some((path, value)) => Loaded {
            path: path.display().to_string(), fallback: false, value: Some(value.clone()), backup: None,
        },
        None => load(),
    }
}

/// 読む。壊れた JSON は .bak へ退避して null を返す。
pub fn load() -> Loaded {
    let (p, fallback) = (path(), place().1);
    let mut out = Loaded { path: p.display().to_string(), fallback, value: None, backup: None };
    let Ok(text) = std::fs::read_to_string(p) else { return out };
    match serde_json::from_str::<Value>(&text) {
        Ok(v) if v.is_object() => out.value = Some(v),
        _ => {
            let bak = p.with_extension("json.bak");
            if std::fs::rename(p, &bak).is_ok() {
                out.backup = Some(bak.display().to_string());
            }
        }
    }
    out
}

/// 書く。途中で落ちても元のファイルが壊れないよう、一時ファイルに書いてから置き換える。
pub fn save(value: &Value) -> Result<(), String> {
    let p = path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, p).map_err(|e| format!("{}: {e}", p.display()))
}

/// 設定ファイルのあるフォルダをエクスプローラーで開き、ファイルを選んだ状態にする。
pub fn reveal() -> Result<(), String> {
    reveal_path(path())
}

/// ファイルをエクスプローラーで選んだ状態にする。無ければそのフォルダを開く。
pub fn reveal_path(p: &Path) -> Result<(), String> {
    let mut cmd = std::process::Command::new("explorer");
    if p.exists() {
        cmd.arg(format!("/select,{}", p.display()));
    } else {
        cmd.arg(p.parent().unwrap_or(p));
    }
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// 起動画面（スプラッシュ）を出すか。設定の splash が false のときだけ出さない。
pub fn splash(settings: &Loaded) -> bool {
    settings.value.as_ref().and_then(|v| v.get("splash")).and_then(Value::as_bool) != Some(false)
}

/// 起動時：保存したウィンドウの位置・大きさ・最大化を戻す（画面のちらつきを避けるため、
/// tauri.conf.json でウィンドウは非表示で作る）。位置がどの画面にも入らなければ既定の位置。
/// show が false のときは表示を画面側の準備の完了（app_ready）まで待つ。非表示のまま最大化すると
/// 表示されるので、そのときは最大化を後回しにして true を返す。
pub fn restore_window<R: Runtime>(win: &WebviewWindow<R>, settings: &Loaded, show: bool) -> bool {
    if let Some(w) = settings.value.as_ref().and_then(|v| v.get("window")) {
        let num = |k: &str| w.get(k).and_then(Value::as_f64);
        if let (Some(x), Some(y), Some(width), Some(height)) = (num("x"), num("y"), num("width"), num("height")) {
            let (x, y, width, height) = (x as i32, y as i32, width as u32, height as u32);
            if width >= 300 && height >= 200 && on_screen(win, x, y, width) {
                let _ = win.set_size(PhysicalSize::new(width, height));
                let _ = win.set_position(PhysicalPosition::new(x, y));
            }
        }
        if w.get("maximized").and_then(Value::as_bool) == Some(true) {
            if !show {
                return true;
            }
            let _ = win.maximize();
        }
    }
    if show {
        let _ = win.show();
    }
    false
}

/// ウィンドウの上端（タイトルバー）のどこかがいずれかの画面に入っているか。
fn on_screen<R: Runtime>(win: &WebviewWindow<R>, x: i32, y: i32, width: u32) -> bool {
    let Ok(monitors) = win.app_handle().available_monitors() else { return false };
    monitors.iter().any(|m| {
        let (p, s) = (m.position(), m.size());
        let (mx0, my0, mx1, my1) = (p.x, p.y, p.x + s.width as i32, p.y + s.height as i32);
        let (cx0, cx1, cy) = (x + 40, x + width as i32 - 40, y + 10);
        cy >= my0 && cy < my1 && cx1 > mx0 && cx0 < mx1
    })
}
