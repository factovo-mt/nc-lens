use std::{
    collections::BTreeMap,
    env,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    process::ExitCode,
    sync::Arc,
    time::Duration,
};

use axum::{
    body::Bytes,
    extract::{rejection::BytesRejection, DefaultBodyLimit, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use nclens_cli::{
    decode, execute, expanded_nc, run_output, wants_trace, AppSwitches, Config, Encoding, Error,
    Include, DEFAULT_INCLUDE,
};
use nclens_core::{
    builtin_dialects, dialect_from_json, macro_reference, make_report, post_from_json,
    post_from_str,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Semaphore;

const USAGE: &str =
    "usage: nclens-server [--host IP] [--port PORT] [--max-body BYTES] [--timeout SECONDS]";

struct Options {
    host: IpAddr,
    port: u16,
    max_body: usize,
    timeout: Duration,
}

impl Options {
    fn parse() -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let mut options = Self {
            host: Ipv4Addr::LOCALHOST.into(),
            port: 8765,
            max_body: 20_000_000,
            timeout: Duration::from_secs(30),
        };
        let mut args = env::args().skip(1);
        while let Some(flag) = args.next() {
            if flag == "--help" || flag == "-h" {
                println!("{USAGE}");
                return Ok(None);
            }
            let value = args.next().ok_or(USAGE)?;
            match flag.as_str() {
                "--host" => options.host = value.parse()?,
                "--port" => options.port = value.parse()?,
                "--max-body" => options.max_body = value.parse()?,
                "--timeout" => options.timeout = Duration::from_secs(value.parse()?),
                _ => return Err(USAGE.into()),
            }
        }
        if options.max_body == 0 || options.timeout.is_zero() {
            return Err("--max-body and --timeout must be greater than zero".into());
        }
        Ok(Some(options))
    }
}

fn main() -> ExitCode {
    match start() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn start() -> Result<(), Box<dyn std::error::Error>> {
    let Some(options) = Options::parse()? else {
        return Ok(());
    };
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async move {
        let address = SocketAddr::new(options.host, options.port);
        if options.host != IpAddr::V4(Ipv4Addr::LOCALHOST) {
            eprintln!("warning: listening on {address} without authentication");
        }
        let listener = tokio::net::TcpListener::bind(address).await?;
        println!("Listening on {}", listener.local_addr()?);
        axum::serve(listener, router(&options)).await?;
        Ok(())
    })
}

fn router(options: &Options) -> Router {
    let jobs = Arc::new(Semaphore::new(
        std::thread::available_parallelism().map_or(1, usize::from),
    ));
    let timeout = options.timeout;
    Router::new()
        .route(
            "/v1/health",
            get(|| async { Json(json!({"status": "ok"})) }),
        )
        .route("/v1/mnemonics", get(|| async { Json(builtin_dialects()) }))
        .route("/v1/macro-reference", post(reference))
        .route("/v1/run", post(run))
        .route("/v1/expand", post(expand))
        .route("/v1/report", post(report))
        .layer(DefaultBodyLimit::max(options.max_body))
        .layer(middleware::from_fn(
            move |req: Request, next: Next| async move {
                if req.headers().contains_key(header::ORIGIN)
                    && req.method() == axum::http::Method::POST
                {
                    return failure(
                        StatusCode::FORBIDDEN,
                        "Browser cross-origin POST is disabled",
                    );
                }
                match tokio::time::timeout(timeout, next.run(req)).await {
                    Ok(response) => response,
                    Err(_) => failure(StatusCode::REQUEST_TIMEOUT, "Request timed out"),
                }
            },
        ))
        .with_state(jobs)
}

async fn reference(body: Bytes) -> Response {
    let settings = std::str::from_utf8(&body)
        .map_err(|error| error.to_string())
        .and_then(|json| dialect_from_json(json).map_err(|error| error.to_string()));
    match settings {
        Ok(settings) => Json(macro_reference(&settings)).into_response(),
        Err(message) => failure(StatusCode::BAD_REQUEST, &message),
    }
}

#[derive(Clone, Copy)]
enum Output {
    Run,
    Expand,
    Report,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunRequest {
    nc: String,
    #[serde(default)]
    encoding: Encoding,
    post: Option<String>,
    block_skip: Option<bool>,
    block_skip_levels: Option<Vec<u8>>,
    optional_stop: Option<bool>,
    #[serde(default)]
    set: BTreeMap<String, Value>,
    include: Option<Vec<Include>>,
}

async fn run(
    State(jobs): State<Arc<Semaphore>>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    handle(Output::Run, jobs, headers, body).await
}

async fn expand(
    State(jobs): State<Arc<Semaphore>>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    handle(Output::Expand, jobs, headers, body).await
}

async fn report(
    State(jobs): State<Arc<Semaphore>>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    handle(Output::Report, jobs, headers, body).await
}

async fn handle(
    output: Output,
    jobs: Arc<Semaphore>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let body = match body {
        Ok(body) => body,
        Err(err) => return failure(err.status(), err.body_text()),
    };
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("text/plain");
    let mut parameters = content_type.split(';');
    let mime = parameters.next().unwrap_or_default().trim();
    let is_json = mime.eq_ignore_ascii_case("application/json");
    if !is_json && !mime.eq_ignore_ascii_case("text/plain") {
        return failure(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Expected text/plain or application/json",
        );
    }
    let charset = parameters
        .filter_map(|entry| entry.trim().split_once('='))
        .find(|(key, _)| key.trim().eq_ignore_ascii_case("charset"))
        .map(|(_, value)| value.trim().trim_matches('"').to_ascii_lowercase());
    let encoding = match charset.as_deref() {
        Some("utf-8" | "utf8") => Encoding::Utf8,
        Some("cp932" | "shift_jis") if !is_json => Encoding::Cp932,
        None => Encoding::Auto,
        _ => return failure(StatusCode::UNSUPPORTED_MEDIA_TYPE, "Unsupported charset"),
    };
    let Ok(permit) = jobs.try_acquire_owned() else {
        return failure(StatusCode::SERVICE_UNAVAILABLE, "Server is busy");
    };
    match tokio::task::spawn_blocking(move || {
        let _permit = permit;
        evaluate(output, &body, is_json, encoding)
    })
    .await
    {
        Ok(Ok(response)) => response,
        Ok(Err(err)) => failure(
            if err.code == 2 {
                StatusCode::UNPROCESSABLE_ENTITY
            } else {
                StatusCode::BAD_REQUEST
            },
            err.message,
        ),
        Err(_) => failure(StatusCode::INTERNAL_SERVER_ERROR, "NC processing failed"),
    }
}

fn evaluate(
    output: Output,
    body: &[u8],
    is_json: bool,
    encoding: Encoding,
) -> Result<Response, Error> {
    let (nc, detected, config, include) = if is_json {
        let body = body.strip_prefix(b"\xef\xbb\xbf").unwrap_or(body);
        let request: RunRequest = serde_json::from_slice(body).map_err(Error::config)?;
        let detected = match request.encoding {
            Encoding::Cp932 => "cp932",
            Encoding::Auto | Encoding::Utf8 => "utf-8",
        };
        let mut post = request
            .post
            .as_deref()
            .map(|src| {
                if src.trim_start().starts_with('{') {
                    post_from_json(src)
                } else {
                    post_from_str(src)
                }
            })
            .transpose()
            .map_err(Error::config)?
            .unwrap_or_default();
        let block_skip = request
            .block_skip
            .or(request.block_skip_levels.as_ref().map(|_| true));
        if let Some(levels) = request.block_skip_levels {
            post.expand.block_skip_levels = levels;
        }
        (
            request.nc,
            detected,
            Config::resolve(
                post,
                AppSwitches {
                    block_skip,
                    optional_stop: request.optional_stop,
                },
                request.set,
            )?,
            request.include.unwrap_or_else(|| DEFAULT_INCLUDE.to_vec()),
        )
    } else {
        let (nc, detected) = decode(body, encoding)?;
        (
            nc.into_owned(),
            detected,
            Config::resolve(Default::default(), Default::default(), [])?,
            DEFAULT_INCLUDE.to_vec(),
        )
    };
    let (result, trace) = execute(
        &nc,
        &config,
        matches!(output, Output::Run) && wants_trace(&include),
    );
    let status = if result.diagnostics.is_empty() {
        StatusCode::OK
    } else {
        StatusCode::UNPROCESSABLE_ENTITY
    };
    let response = match output {
        Output::Run => {
            Json(run_output(&result, trace, &config, "-", detected, &include)).into_response()
        }
        Output::Expand if status == StatusCode::OK => expanded_nc(&result).into_response(),
        Output::Report => Json(make_report(&result, "-")).into_response(),
        Output::Expand => Json(json!({
            "message": "NC has errors",
            "diagnostics": result.diagnostics, "warnings": result.warnings,
        }))
        .into_response(),
    };
    Ok((status, response).into_response())
}

fn failure(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({"message": message.into()}))).into_response()
}
