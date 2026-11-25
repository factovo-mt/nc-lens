use std::{
    fmt::{self, Write as _},
    fs, io,
    path::Path,
};

use nclens_core::{
    builtin_dialects, load_dialect, load_post, make_report, run_with_post_skip_levels,
    run_with_post_trace_and_skip_levels, PostProfile, RunResult,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub struct Error {
    pub code: u8,
    pub message: String,
}

impl Error {
    pub fn config(error: impl fmt::Display) -> Self {
        Self {
            code: 3,
            message: error.to_string(),
        }
    }

    pub fn input(error: impl fmt::Display) -> Self {
        Self {
            code: 2,
            message: error.to_string(),
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self {
            code: 4,
            message: error.to_string(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Include {
    Report,
    Expanded,
    Trace,
    Path,
}

pub const DEFAULT_INCLUDE: &[Include] = &[Include::Report, Include::Expanded];

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub enum Encoding {
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "utf-8", alias = "utf8")]
    Utf8,
    #[serde(rename = "cp932", alias = "shift_jis")]
    Cp932,
}

pub fn decode(
    bytes: &[u8],
    encoding: Encoding,
) -> Result<(std::borrow::Cow<'_, str>, &'static str), Error> {
    if !matches!(encoding, Encoding::Cp932) {
        let utf8 = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
        match std::str::from_utf8(utf8) {
            Ok(text) => return Ok((text.into(), "utf-8")),
            Err(err) if matches!(encoding, Encoding::Utf8) => return Err(Error::input(err)),
            Err(_) => {}
        }
    }
    encoding_rs::SHIFT_JIS
        .decode_without_bom_handling_and_without_replacement(bytes)
        .map(|text| (text, "cp932"))
        .ok_or_else(|| Error::input("Invalid CP932 input"))
}

pub fn read_text(path: &Path) -> Result<String, Error> {
    let bytes = fs::read(path).map_err(|err| Error {
        code: 4,
        message: format!("{}: {err}", path.display()),
    })?;
    String::from_utf8(
        bytes
            .strip_prefix(b"\xef\xbb\xbf")
            .unwrap_or(&bytes)
            .to_vec(),
    )
    .map_err(|err| Error::config(format!("{}: {err}", path.display())))
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppSwitches {
    pub block_skip: Option<bool>,
    pub optional_stop: Option<bool>,
}

pub fn read_settings(path: &Path) -> Result<Value, Error> {
    let settings: Value = serde_json::from_str(&read_text(path)?).map_err(Error::config)?;
    settings_switches(&settings)?;
    settings_levels(&settings)?;
    Ok(settings)
}

pub fn settings_switches(value: &Value) -> Result<AppSwitches, Error> {
    if !value.is_object() {
        return Err(Error::config("Settings must be a JSON object"));
    }
    if value.get("version").is_some_and(|v| v.as_u64() != Some(1)) {
        return Err(Error::config("Unsupported settings version"));
    }
    serde_json::from_value(value.clone()).map_err(Error::config)
}

fn settings_levels(value: &Value) -> Result<Option<Vec<u8>>, Error> {
    let Some(levels) = value
        .get("blockSkipLevels")
        .filter(|value| !value.is_null())
    else {
        return Ok(None);
    };
    let levels: Vec<u8> = serde_json::from_value(levels.clone()).map_err(Error::config)?;
    if levels.iter().any(|level| !(1..=9).contains(level)) {
        return Err(Error::config(
            "blockSkipLevels must contain numbers 1 through 9",
        ));
    }
    Ok(Some(levels))
}

pub fn parse_set(item: &str) -> Result<(String, Value), Error> {
    let (key, raw) = item
        .split_once('=')
        .filter(|(key, _)| !key.is_empty())
        .ok_or_else(|| Error::config(format!("Expected KEY=VALUE: {item}")))?;
    Ok((
        key.into(),
        serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.into())),
    ))
}

pub struct Config {
    pub post: PostProfile,
    pub optional_stop: bool,
}

impl Config {
    pub fn resolve_settings(
        mut post: PostProfile,
        settings: &Value,
        overrides: impl IntoIterator<Item = (String, Value)>,
    ) -> Result<Self, Error> {
        let switches = settings_switches(settings)?;
        if let Some(levels) = settings_levels(settings)? {
            post.expand.block_skip_levels = levels;
        }
        Self::resolve(post, switches, overrides)
    }

    pub fn resolve(
        mut post: PostProfile,
        switches: AppSwitches,
        overrides: impl IntoIterator<Item = (String, Value)>,
    ) -> Result<Self, Error> {
        post.expand.block_skip = switches.block_skip.unwrap_or(post.expand.block_skip);
        let mut optional_stop = switches.optional_stop.unwrap_or(false);
        let mut profile = serde_json::to_value(&post).map_err(Error::config)?;
        for (key, value) in overrides {
            match key.as_str() {
                "optionalStop" | "expand.optional_stop" => {
                    optional_stop = value
                        .as_bool()
                        .ok_or_else(|| Error::config(format!("{key} must be a boolean")))?;
                }
                "blockSkip" => set_value(&mut profile, "expand.block_skip", value)?,
                "blockSkipLevels" => set_value(&mut profile, "expand.block_skip_levels", value)?,
                _ => set_value(&mut profile, &key, value)?,
            }
        }
        let post: PostProfile = serde_json::from_value(profile).map_err(Error::config)?;
        post.validate()
            .map_err(|field| Error::config(format!("Invalid post setting {}", field.join("."))))?;
        Ok(Self {
            post,
            optional_stop,
        })
    }
}

fn set_value(root: &mut Value, key: &str, value: Value) -> Result<(), Error> {
    let key = if key == "codes.rapid_feed" {
        "codes.rapid_mm_min"
    } else {
        key
    };
    let mut fields: Vec<_> = key.split('.').map(str::to_owned).collect();
    if let [group, table, work, axis] = fields.as_mut_slice() {
        if group == "variables" && table == "work" {
            if let Some(index) = ["G54", "G55", "G56", "G57", "G58", "G59"]
                .iter()
                .position(|s| work.eq_ignore_ascii_case(s))
            {
                *work = index.to_string();
            }
            if let Some(index) = ["x", "y", "z"]
                .iter()
                .position(|s| axis.eq_ignore_ascii_case(s))
            {
                *axis = index.to_string();
            }
        }
    }
    let invalid = || Error::config(format!("Invalid setting path: {key}"));
    let (last, parents) = fields.split_last().ok_or_else(invalid)?;
    if fields.iter().any(String::is_empty) {
        return Err(invalid());
    }
    let mut parent = root;
    for field in parents {
        parent = match parent {
            Value::Array(items) => field
                .parse::<usize>()
                .ok()
                .and_then(|i| items.get_mut(i))
                .ok_or_else(invalid)?,
            Value::Object(items) => items.entry(field.clone()).or_insert_with(|| json!({})),
            _ => return Err(invalid()),
        };
    }
    match parent {
        Value::Array(items) => {
            *last
                .parse::<usize>()
                .ok()
                .and_then(|i| items.get_mut(i))
                .ok_or_else(invalid)? = value;
        }
        Value::Object(items) => {
            items.insert(last.clone(), value);
        }
        _ => return Err(invalid()),
    }
    Ok(())
}

pub fn resolve_files(
    post: Option<&Path>,
    dialect: Option<&Path>,
    settings: Option<&Path>,
    overrides: impl IntoIterator<Item = (String, Value)>,
) -> Result<Config, Error> {
    let mut profile = match post {
        Some(path) => {
            load_post(path).map_err(|err| Error::config(format!("{}: {err}", path.display())))?
        }
        None => PostProfile::default(),
    };
    if let Some(selection) = dialect {
        profile.dialect = match builtin_dialects()
            .into_iter()
            .find(|d| selection.as_os_str() == d.name.as_str())
        {
            Some(dialect) => dialect,
            None => load_dialect(selection)
                .map_err(|err| Error::config(format!("{}: {err}", selection.display())))?,
        };
    }
    if let Some(path) = settings {
        Config::resolve_settings(profile, &read_settings(path)?, overrides)
    } else {
        Config::resolve(profile, AppSwitches::default(), overrides)
    }
}

pub fn execute(text: &str, config: &Config, trace: bool) -> (RunResult, Vec<Value>) {
    let mut entries = Vec::new();
    let result = if trace {
        execute_trace(text, config, &[], |entry| entries.push(entry))
    } else {
        with_feed_warnings(run_with_post_skip_levels(
            text,
            &config.post,
            config.post.expand.enabled_block_skip_levels(),
            config.optional_stop,
        ))
    };
    (result, entries)
}

pub fn execute_trace(
    text: &str,
    config: &Config,
    watch: &[u32],
    mut emit: impl FnMut(Value),
) -> RunResult {
    let normalized = text.replace("\r\n", "\n");
    let source: Vec<_> = normalized.split(['\r', '\n']).collect();
    let mut seq = 0;
    with_feed_warnings(run_with_post_trace_and_skip_levels(
        text,
        &config.post,
        config.post.expand.enabled_block_skip_levels(),
        config.optional_stop,
        |line, stack, changes| {
            seq += 1;
            let writes: Vec<_> = changes
                .iter()
                .filter(|v| watch.is_empty() || watch.contains(&v.var))
                .collect();
            if watch.is_empty() || !writes.is_empty() {
                emit(json!({
                    "seq": seq, "src_line": line, "call_stack": stack,
                    "source": source.get(line.saturating_sub(1)).copied().unwrap_or_default(),
                    "writes": writes,
                }));
            }
        },
    ))
}

// The core report counts untimed cuts, but those warnings have no diagnostic entries.
fn with_feed_warnings(mut result: RunResult) -> RunResult {
    let mut emitted = std::collections::BTreeSet::new();
    for segment in &result.path {
        if segment.kind == nclens_core::SegKind::Cut
            && segment.time_sec == 0.
            && emitted.insert(segment.src_line)
        {
            result.warnings.push(nclens_core::Diagnostic {
                src_line: segment.src_line,
                message: "Cut has no calculated machining time".into(),
            });
        }
    }
    result
}

pub fn status(result: &RunResult) -> u8 {
    if !result.diagnostics.is_empty() {
        2
    } else if !result.warnings.is_empty() {
        1
    } else {
        0
    }
}

pub fn expanded_nc(result: &RunResult) -> String {
    let mut out = String::from("%\n");
    for line in &result.expanded {
        out.push_str(&line.text);
        out.push('\n');
    }
    out.push_str("%\n");
    out
}

pub fn run_output(
    result: &RunResult,
    trace: Vec<Value>,
    config: &Config,
    filename: &str,
    encoding: &str,
    include: &[Include],
) -> Value {
    let profile = serde_json::to_vec(&config.post).expect("serializable post");
    let mut hash = String::with_capacity(64);
    for byte in Sha256::digest(profile) {
        write!(hash, "{byte:02x}").expect("write to String");
    }
    let mut output = json!({
        "schema_version": 1,
        "input": { "filename": filename, "encoding": encoding },
        "post": { "name": config.post.name, "hash": hash },
        "diagnostics": result.diagnostics, "warnings": result.warnings,
        "tools": result.tools, "stops": result.stops,
    });
    if !result.block_skip_usage.is_empty() {
        output["block_skip_usage"] = json!(result.block_skip_usage);
    }
    for section in include {
        match section {
            Include::Report => output["report"] = json!(make_report(result, filename)),
            Include::Expanded => output["expanded"] = json!(result.expanded),
            Include::Trace => output["trace"] = json!(trace),
            Include::Path => output["path"] = json!(result.path),
        }
    }
    output
}

pub fn wants_trace(include: &[Include]) -> bool {
    include.iter().any(|item| matches!(item, Include::Trace))
}
