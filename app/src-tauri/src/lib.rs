//! GUI の Rust 側。業務の処理は nclens-core に任せ、ここは受け渡しだけ。

mod samples;
mod settings;
mod startup;

use nclens_core::{DialectSettings, PostProfile, Report, RunResult};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

type Startup = Result<startup::Startup, nclens_cli::Error>;

#[tauri::command]
fn startup(
    args: tauri::State<Startup>,
    post: Option<PostProfile>,
    switches: nclens_cli::AppSwitches,
) -> Result<startup::Launch, String> {
    args.inner().as_ref().map_err(|err| err.to_string())?
        .resolve(post, switches).map_err(|err| err.to_string())
}

/// 最後に実行した結果のレポート（レポート画面が last_report で読む）。
#[derive(Default)]
struct LastReport(Mutex<Option<Report>>);

/// NC テキストを実行し、結果をそのまま画面へ返す。post が無ければ既定のポスト。
/// skip_levels は有効なブロックスキップの番号（空はスキップ無し、無ければポストの既定）。
/// レポートは RunResult の内側の集計から作るので、ここで作って取っておく。
#[tauri::command]
fn run_nc(
    text: String,
    post: Option<PostProfile>,
    skip_levels: Option<Vec<u8>>,
    last: tauri::State<LastReport>,
) -> RunResult {
    let post = post.unwrap_or_default();
    let levels = skip_levels.as_deref().unwrap_or_else(|| post.expand.enabled_block_skip_levels());
    let result = nclens_core::run_through_stops_with_skip_levels(&text, &post, levels);
    *last.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(nclens_core::make_report(&result, ""));
    result
}

/// 最後に実行した結果のレポート。filename は画面側で入れる。
#[tauri::command]
fn last_report(last: tauri::State<LastReport>) -> Option<Report> {
    last.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// 既定のポスト（新規）。
#[tauri::command]
fn post_default() -> PostProfile {
    PostProfile::default()
}

/// ポスト（TOML・JSON）を読む。誤りは行番号つきの文言で返す。
#[tauri::command]
fn post_load(path: String) -> Result<PostProfile, String> {
    nclens_core::load_post(&path).map_err(|e| e.to_string())
}

/// ポストを書く（拡張子 .toml・.json で形式を選ぶ。コア側で中身を検査してから書く）。
#[tauri::command]
fn post_save(path: String, post: PostProfile) -> Result<(), String> {
    nclens_core::save_post(&path, &post).map_err(|e| e.to_string())
}

/// 組み込みの方言（FANUC・OSP）。
#[tauri::command]
fn dialect_builtin() -> Vec<DialectSettings> {
    nclens_core::builtin_dialects()
}

/// 方言（TOML・JSON）を読む。
#[tauri::command]
fn dialect_load(path: String) -> Result<DialectSettings, String> {
    nclens_core::load_dialect(&path).map_err(|e| e.to_string())
}

/// 方言を書く（拡張子 .toml・.json で形式を選ぶ。コア側で中身を検査してから書く）。
#[tauri::command]
fn dialect_save(path: String, dialect: DialectSettings) -> Result<(), String> {
    nclens_core::save_dialect(&path, &dialect).map_err(|e| e.to_string())
}

/// 方言の誤り（空なら正しい）。
#[tauri::command]
fn dialect_validate(dialect: DialectSettings) -> Vec<String> {
    nclens_core::validate_dialect(&dialect)
}

/// 読んだファイルの中身と文字コード（"utf-8" か "cp932"）。保存のときに同じ文字コードで書き戻す。
#[derive(Serialize)]
struct OpenedFile {
    text: String,
    encoding: &'static str,
}

/// ファイルを読む。UTF-8 として正しければ UTF-8、そうでなければ CP932 とみなす。
#[tauri::command]
fn open_file(path: String) -> Result<OpenedFile, String> {
    let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    let (text, encoding) = decode(&bytes);
    Ok(OpenedFile { text, encoding })
}

/// ファイルを書く。改行は画面側でそろえて渡す。encoding は "utf-8" か "cp932"。
#[tauri::command]
fn save_file(path: String, text: String, encoding: String) -> Result<(), String> {
    let bytes = encode(&text, &encoding)?;
    std::fs::write(&path, bytes).map_err(|e| format!("{path}: {e}"))
}

/// アプリの設定（nclens.settings.json）を読む。
#[tauri::command]
fn settings_load(args: tauri::State<Startup>) -> settings::Loaded {
    settings::load_for(args.inner().as_ref().ok())
}

/// アプリの設定を書く。
#[tauri::command]
fn settings_save(value: serde_json::Value, args: tauri::State<Startup>) -> Result<(), String> {
    if args.inner().as_ref().is_ok_and(startup::Startup::session_config) {
        return Ok(());
    }
    settings::save(&value)
}

/// 設定ファイルのあるフォルダを開く。
#[tauri::command]
fn settings_reveal() -> Result<(), String> {
    settings::reveal()
}

/// 同梱の第三者のライセンス（exe の横の THIRD_PARTY_LICENSES.md）をエクスプローラーで選ぶ。
#[tauri::command]
fn licenses_reveal() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("exe のフォルダが見つかりません")?;
    settings::reveal_path(&dir.join("THIRD_PARTY_LICENSES.md"))
}

/// 公開用のリンク（app/src/links.ts）を既定のブラウザで開く。公開のサイト・リポジトリ以外は開かない。
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !url_allowed(&url) {
        return Err(format!("開けない URL：{url}"));
    }
    std::process::Command::new("explorer").arg(&url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

fn url_allowed(url: &str) -> bool {
    const ALLOWED: [&str; 2] = ["https://nclens.factovo-works.com", "https://github.com/factovo-mt/nc-lens"];
    ALLOWED.iter().any(|a| url == *a || url.strip_prefix(a).is_some_and(|r| r.starts_with('/')))
        && !url.contains(['"', ' ', '\\'])
}

/// 同梱のサンプルのフォルダ（開く・既定のフォルダの初期値）。
#[tauri::command]
fn samples_dir() -> Option<String> {
    samples::dir().map(|p| p.display().to_string())
}

/// 同梱のサンプルの一覧（ファイル → サンプルを開く）。
#[tauri::command]
fn samples_list() -> Vec<samples::Sample> {
    samples::list()
}

/// 起動画面が初めて描かれた時刻（splash_shown。最短の表示時間をここから数える）
static SPLASH_SHOWN: OnceLock<Instant> = OnceLock::new();
/// 起動画面の最短の表示時間。準備がこれより早く済んでも、ちらつかないよう少しだけ見せる
const SPLASH_MIN: Duration = Duration::from_millis(800);
/// 画面側が準備の完了を知らせないとき（読み込みの失敗など）でも、本体を出すまでの上限
const SPLASH_MAX: Duration = Duration::from_secs(15);
/// 本体を非表示のまま最大化すると表示されるため、最大化は表示のときに行う
static PENDING_MAXIMIZE: AtomicBool = AtomicBool::new(false);

/// 起動画面を閉じて本体を表示する（2 度目以降は何もしない）。
fn show_main(app: &AppHandle) {
    let Some(main) = app.get_webview_window("main") else { return };
    if PENDING_MAXIMIZE.swap(false, Ordering::SeqCst) {
        let _ = main.maximize();
    }
    let _ = main.show();
    if let Some(splash) = app.get_webview_window("splash") {
        let _ = main.set_focus();
        let _ = splash.close();
    }
}

/// 起動画面が描かれた（splash.ts が最初の描画の後に呼ぶ）。
#[tauri::command]
fn splash_shown() {
    SPLASH_SHOWN.get_or_init(Instant::now);
}

/// 画面側の準備の完了。起動画面があれば、描かれてから最短の表示時間まで待って閉じ、本体を表示する。
/// 起動画面の描画が遅れても（WebView2 の初回の起動など）待つのは 1.5 秒まで。
#[tauri::command]
async fn app_ready(app: AppHandle) {
    if app.get_webview_window("splash").is_some() {
        let start = Instant::now();
        while SPLASH_SHOWN.get().is_none() && start.elapsed() < Duration::from_millis(1500) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let shown = SPLASH_SHOWN.get().map_or(SPLASH_MIN, Instant::elapsed);
        if shown < SPLASH_MIN {
            std::thread::sleep(SPLASH_MIN - shown);
        }
    }
    show_main(&app);
}

/// 起動画面：枠なし・透過・中央の小さなウィンドウ（splash.html）。
fn open_splash(app: &tauri::App) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, "splash", WebviewUrl::App("splash.html".into()))
        .title("NC Lens")
        .inner_size(460., 300.)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .skip_taskbar(true)
        .center()
        .build()?;
    let handle = app.handle().clone();
    std::thread::spawn(move || {
        std::thread::sleep(SPLASH_MAX);
        let main = handle.clone();
        let _ = handle.run_on_main_thread(move || show_main(&main));
    });
    Ok(())
}

fn decode(bytes: &[u8]) -> (String, &'static str) {
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_owned(), "utf-8"),
        Err(_) => (encoding_rs::SHIFT_JIS.decode(bytes).0.into_owned(), "cp932"),
    }
}

fn encode(text: &str, encoding: &str) -> Result<Vec<u8>, String> {
    match encoding {
        "utf-8" => Ok(text.as_bytes().to_vec()),
        "cp932" => {
            let (bytes, _, unmappable) = encoding_rs::SHIFT_JIS.encode(text);
            if unmappable {
                return Err("CP932 で表せない文字があります（UTF-8 で保存してください）".into());
            }
            Ok(bytes.into_owned())
        }
        _ => Err(format!("未対応の文字コード：{encoding}")),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(LastReport::default())
        .manage(startup::Startup::parse(std::env::args_os().skip(1)))
        .setup(|app| {
            let loaded = settings::load_for(app.state::<Startup>().inner().as_ref().ok());
            let splash = settings::splash(&loaded) && open_splash(app).is_ok();
            if let Some(win) = app.get_webview_window("main") {
                let max = settings::restore_window(&win, &loaded, !splash);
                PENDING_MAXIMIZE.store(max, Ordering::SeqCst);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            run_nc,
            last_report,
            open_file,
            save_file,
            post_default,
            post_load,
            post_save,
            dialect_builtin,
            dialect_load,
            dialect_save,
            dialect_validate,
            startup,
            settings_load,
            settings_save,
            settings_reveal,
            samples_dir,
            samples_list,
            licenses_reveal,
            open_url,
            app_ready,
            splash_shown
        ])
        .run(tauri::generate_context!())
        .expect("Tauri の起動に失敗");
}

#[cfg(test)]
mod tests {
    use super::{decode, encode, url_allowed};

    #[test]
    fn open_url_allowlist() {
        assert!(url_allowed("https://nclens.factovo-works.com"));
        assert!(url_allowed("https://github.com/factovo-mt/nc-lens/releases/latest"));
        assert!(!url_allowed("https://github.com/factovo-mt/nc-lens-evil"));
        assert!(!url_allowed("https://nclens.factovo-works.com.evil.example"));
        assert!(!url_allowed("file:///C:/Windows"));
    }

    #[test]
    fn decode_cp932() {
        // 「穴」の CP932 は 0x8C 0x8A
        assert_eq!(decode(&[b'(', 0x8C, 0x8A, b')']), ("(穴)".into(), "cp932"));
        assert_eq!(decode("(穴)".as_bytes()), ("(穴)".into(), "utf-8"));
    }

    #[test]
    fn encode_round_trip() {
        assert_eq!(encode("(穴)\r\n", "cp932").unwrap(), [b'(', 0x8C, 0x8A, b')', b'\r', b'\n']);
        assert_eq!(encode("(穴)", "utf-8").unwrap(), "(穴)".as_bytes());
        assert!(encode("(😀)", "cp932").is_err());
    }
}
