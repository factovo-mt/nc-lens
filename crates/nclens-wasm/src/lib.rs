//! ブラウザと nclens-core の間で文字列とバイト列を受け渡す。

use nclens_core::{PostProfile, Report, RunResult};
use serde::{de::DeserializeOwned, Serialize};
use wasm_bindgen::prelude::*;

fn js_error(error: impl std::fmt::Display) -> JsError {
    JsError::new(&error.to_string())
}

fn json(value: &impl Serialize) -> Result<String, JsError> {
    serde_json::to_string(value).map_err(js_error)
}

fn from_json<T: DeserializeOwned>(value: &str) -> Result<T, JsError> {
    serde_json::from_str(value).map_err(js_error)
}

#[wasm_bindgen]
pub fn run(src: &str) -> Result<String, JsError> {
    json(&nclens_core::run(src))
}

#[wasm_bindgen]
pub fn run_with(src: &str, post: &str) -> Result<String, JsError> {
    json(&nclens_core::run_with(src, &from_json(post)?))
}

/// M00・M01 の停止位置（stops）、呼び出し（calls）、元の G コード（source_g_codes）を含めて実行する。
#[wasm_bindgen]
pub fn run_through_stops(src: &str, post: &str) -> Result<String, JsError> {
    let post: Option<PostProfile> = from_json(post)?;
    json(&nclens_core::run_through_stops(
        src,
        &post.unwrap_or_default(),
    ))
}

#[wasm_bindgen]
pub fn make_report(
    src: &str,
    post: &str,
    filename: &str,
    timestamp: u64,
) -> Result<String, JsError> {
    let post: Option<PostProfile> = from_json(post)?;
    let result = nclens_core::run_with(src, &post.unwrap_or_default());
    json(&nclens_core::make_report_at(&result, filename, timestamp))
}

#[derive(Serialize)]
struct Execution {
    result: RunResult,
    report: Report,
}

#[wasm_bindgen]
pub fn run_nc(
    src: &str,
    post: &str,
    timestamp: u64,
    block_skip: Option<bool>,
) -> Result<String, JsError> {
    let post: Option<PostProfile> = from_json(post)?;
    let post = post.unwrap_or_default();
    let levels: &[u8] = match block_skip {
        Some(true) => &[1],
        Some(false) => &[],
        None => post.expand.enabled_block_skip_levels(),
    };
    execute_nc(src, &post, timestamp, levels)
}

/// 有効なスキップ番号を指定し、停止位置を記録して最後まで展開する。
#[wasm_bindgen]
pub fn run_nc_with_skip_levels(
    src: &str,
    post: &str,
    timestamp: u64,
    levels: &[u8],
) -> Result<String, JsError> {
    let post: Option<PostProfile> = from_json(post)?;
    if levels.iter().any(|level| !(1..=9).contains(level)) {
        return Err(js_error("Expected block skip levels 1 through 9"));
    }
    execute_nc(src, &post.unwrap_or_default(), timestamp, levels)
}

fn execute_nc(
    src: &str,
    post: &PostProfile,
    timestamp: u64,
    levels: &[u8],
) -> Result<String, JsError> {
    let result = nclens_core::run_through_stops_with_skip_levels(src, post, levels);
    let report = nclens_core::make_report_at(&result, "", timestamp);
    json(&Execution { result, report })
}

#[wasm_bindgen]
pub fn post_default() -> Result<String, JsError> {
    json(&PostProfile::default())
}

#[wasm_bindgen]
pub fn parse_post(src: &str) -> Result<String, JsError> {
    json(&nclens_core::post_from_str(src).map_err(js_error)?)
}

#[wasm_bindgen]
pub fn format_post(src: &str) -> Result<String, JsError> {
    nclens_core::post_to_string(&nclens_core::post_from_json(src).map_err(js_error)?)
        .map_err(js_error)
}

#[wasm_bindgen]
pub fn builtin_dialects() -> Result<String, JsError> {
    json(&nclens_core::builtin_dialects())
}

#[wasm_bindgen]
pub fn parse_dialect(src: &str) -> Result<String, JsError> {
    json(&nclens_core::dialect_from_str(src).map_err(js_error)?)
}

#[wasm_bindgen]
pub fn format_dialect(src: &str) -> Result<String, JsError> {
    nclens_core::dialect_to_string(&nclens_core::dialect_from_json(src).map_err(js_error)?)
        .map_err(js_error)
}

#[wasm_bindgen]
pub fn validate_dialect(src: &str) -> Result<String, JsError> {
    json(&nclens_core::validate_dialect(&from_json(src)?))
}

#[derive(Serialize)]
struct OpenedFile<'a> {
    text: std::borrow::Cow<'a, str>,
    encoding: &'static str,
}

#[wasm_bindgen]
pub fn decode_file(bytes: &[u8]) -> Result<String, JsError> {
    let (text, encoding) = match std::str::from_utf8(bytes) {
        Ok(text) => (text.into(), "utf-8"),
        Err(_) => (encoding_rs::SHIFT_JIS.decode(bytes).0, "cp932"),
    };
    json(&OpenedFile { text, encoding })
}

#[wasm_bindgen]
pub fn encode_file(text: &str, encoding: &str) -> Result<Vec<u8>, JsError> {
    match encoding {
        "utf-8" => Ok(text.as_bytes().to_vec()),
        "cp932" => {
            let (bytes, _, unmappable) = encoding_rs::SHIFT_JIS.encode(text);
            if unmappable {
                return Err(js_error(
                    "CP932 で表せない文字があります（UTF-8 で保存してください）",
                ));
            }
            Ok(bytes.into_owned())
        }
        _ => Err(js_error(format!("未対応の文字コード：{encoding}"))),
    }
}

#[wasm_bindgen]
pub fn parse_post_json(src: &str) -> Result<String, JsError> {
    json(&nclens_core::post_from_json(src).map_err(js_error)?)
}

#[wasm_bindgen]
pub fn format_post_json(src: &str) -> Result<String, JsError> {
    nclens_core::post_to_json(&nclens_core::post_from_json(src).map_err(js_error)?)
        .map_err(js_error)
}

#[wasm_bindgen]
pub fn parse_dialect_json(src: &str) -> Result<String, JsError> {
    json(&nclens_core::dialect_from_json(src).map_err(js_error)?)
}

#[wasm_bindgen]
pub fn format_dialect_json(src: &str) -> Result<String, JsError> {
    nclens_core::dialect_to_json(&nclens_core::dialect_from_json(src).map_err(js_error)?)
        .map_err(js_error)
}
