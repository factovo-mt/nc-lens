mod batch;

use std::{
    env,
    ffi::OsString,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use nclens_cli::{
    decode, execute, execute_trace, expanded_nc, parse_set, read_settings, resolve_files,
    run_output, status, wants_trace, Config, Encoding, Error, Include, DEFAULT_INCLUDE,
};
use nclens_core::{
    macro_reference, make_report, post_to_json, post_to_string, PostProfile, DEFAULT_POST_TOML,
};
use serde_json::{json, Value};

const USAGE: &str = "\
usage: nclens-cli <run|expand|report|trace> <NC|-> [--out FILE] [--encoding auto|cp932|utf8]
       nclens-cli <run|expand|report> <file|dir|glob>... --out-dir DIR
       nclens-cli watch DIR --out-dir DIR
       nclens-cli post <init|check> FILE | nclens-cli post show [--format toml|json]
       nclens-cli settings <init|check|show> [FILE]
       nclens-cli macro-reference [--mnemonic fanuc|osp|FILE]
options: --post FILE --mnemonic fanuc|osp|FILE --settings FILE --set KEY=VALUE
         --include report,expanded,trace,path (run) --json (report/post show)
         --map FILE (expand) --watch #n,#n (trace) --block-skip --block-skip-levels 1,3,5 --optional-stop
         --error-format text|json";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Run,
    Expand,
    Report,
    Trace,
    Watch,
    Post,
    Settings,
    MacroReference,
}

#[derive(Default)]
struct Options {
    inputs: Vec<PathBuf>,
    post: Option<PathBuf>,
    dialect: Option<PathBuf>,
    settings: Option<PathBuf>,
    out: Option<PathBuf>,
    out_dir: Option<PathBuf>,
    map: Option<PathBuf>,
    encoding: Encoding,
    include: Option<Vec<Include>>,
    overrides: Vec<(String, Value)>,
    watch: Vec<u32>,
    json: bool,
    post_format: Option<bool>,
    error_json: bool,
}

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let error_json = args
        .windows(2)
        .any(|pair| pair[0] == "--error-format" && pair[1] == "json");
    match command(&args) {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            emit_error(&err, None, error_json);
            ExitCode::from(err.code)
        }
    }
}

fn command(args: &[OsString]) -> Result<u8, Error> {
    let bad_args = || Error::config(USAGE);
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{USAGE}");
        return Ok(0);
    }
    let (verb, rest) = args.split_first().ok_or_else(bad_args)?;
    let action = match verb.to_str() {
        Some("run") => Action::Run,
        Some("expand") => Action::Expand,
        Some("report") => Action::Report,
        Some("trace") => Action::Trace,
        Some("watch") => Action::Watch,
        Some("post") => Action::Post,
        Some("settings") => Action::Settings,
        Some("macro-reference") => Action::MacroReference,
        _ => return Err(bad_args()),
    };
    let (subcommand, rest) = if matches!(action, Action::Post | Action::Settings) {
        let (subcommand, rest) = rest.split_first().ok_or_else(bad_args)?;
        (subcommand.to_str(), rest)
    } else {
        (None, rest)
    };
    if action == Action::MacroReference {
        let selection = match rest {
            [] => None,
            [flag, path] if flag == "--mnemonic" => Some(Path::new(path)),
            _ => return Err(bad_args()),
        };
        let config = resolve_files(None, selection, None, [])?;
        write_json(
            &mut io::stdout().lock(),
            &macro_reference(&config.post.dialect),
        )?;
        return Ok(0);
    }
    let options = parse(action, rest)?;
    if options.out.is_some() && options.out == options.map {
        return Err(Error::config("--out and --map require different files"));
    }
    if matches!(action, Action::Post | Action::Settings) {
        return configuration(action, subcommand, &options);
    }
    if let Some(dir) = &options.out_dir {
        if options.inputs.is_empty()
            || options.out.is_some()
            || options.map.is_some()
            || action == Action::Trace
        {
            return Err(bad_args());
        }
        let config = options.config()?;
        if action == Action::Watch {
            let [input] = &options.inputs[..] else {
                return Err(bad_args());
            };
            return batch::watch(input, dir, &options, &config);
        }
        return batch::run(action, dir, &options, &config);
    }
    let [input] = &options.inputs[..] else {
        return Err(bad_args());
    };
    if action == Action::Watch {
        return Err(bad_args());
    }
    let config = options.config()?;
    let source = read_source(input, options.encoding)?;
    if action == Action::Trace {
        let mut out = output(options.out.as_deref())?;
        let mut write = Ok(());
        let result = execute_trace(&source.text, &config, &options.watch, |entry| {
            if write.is_ok() {
                write = writeln!(out, "{entry}");
            }
        });
        emit_diagnostics(&result, &source.filename, options.error_json)?;
        write?;
        out.flush()?;
        return Ok(status(&result));
    }
    let (result, trace) = execute(&source.text, &config, wants_trace(options.include()));
    emit_diagnostics(&result, &source.filename, options.error_json)?;
    let filename = &source.filename;
    let write = |out: &mut dyn Write| match action {
        Action::Run => write_json(
            out,
            &run_output(
                &result,
                trace,
                &config,
                filename,
                source.encoding,
                options.include(),
            ),
        ),
        Action::Expand => {
            out.write_all(expanded_nc(&result).as_bytes())?;
            if let Some(path) = &options.map {
                let entries: Vec<_> = result.expanded.iter().enumerate().map(|(i, line)| {
                    json!({"out_line": i + 2, "src_line": line.src_line, "call_stack": line.call_stack})
                }).collect();
                batch::write_atomic(path, |file| write_json(file, &entries))?;
            }
            Ok(())
        }
        Action::Report if options.json => write_json(out, &make_report(&result, filename)),
        Action::Report => write_json(out, &result.stats),
        _ => unreachable!(),
    };
    if let Some(path) = &options.out {
        batch::write_atomic(path, |file| write(file))?;
    } else {
        let mut out = io::stdout().lock();
        write(&mut out)?;
        out.flush()?;
    }
    Ok(status(&result))
}

impl Options {
    fn config(&self) -> Result<Config, Error> {
        resolve_files(
            self.post.as_deref(),
            self.dialect.as_deref(),
            self.settings.as_deref(),
            self.overrides.clone(),
        )
    }

    fn include(&self) -> &[Include] {
        self.include.as_deref().unwrap_or(DEFAULT_INCLUDE)
    }
}

fn parse(action: Action, args: &[OsString]) -> Result<Options, Error> {
    let data_command = !matches!(action, Action::Post | Action::Settings);
    let mut options = Options::default();
    let mut args = args.iter();
    let mut positional = false;
    while let Some(flag) = args.next() {
        if positional || !flag.to_string_lossy().starts_with("--") {
            options.inputs.push(flag.into());
            continue;
        }
        match flag.to_str() {
            Some("--") => {
                positional = true;
                continue;
            }
            Some("--json") if matches!(action, Action::Report | Action::Post) => {
                options.json = true;
                continue;
            }
            Some("--block-skip" | "--optional-stop") if data_command => {
                let key = if flag == "--block-skip" {
                    "expand.block_skip"
                } else {
                    "expand.optional_stop"
                };
                if flag == "--block-skip" {
                    options
                        .overrides
                        .push(("expand.block_skip_levels".into(), json!([1])));
                }
                options.overrides.push((key.into(), json!(true)));
                continue;
            }
            _ => {}
        }
        let value = args.next().ok_or_else(|| {
            Error::config(format!("Missing value for {}", flag.to_string_lossy()))
        })?;
        let utf8 = || {
            value
                .to_str()
                .ok_or_else(|| Error::config("Expected UTF-8 argument"))
        };
        match flag.to_str() {
            Some("--format") if action == Action::Post && options.post_format.is_none() => {
                options.post_format = Some(match utf8()? {
                    "toml" => false,
                    "json" => true,
                    _ => return Err(Error::config("Expected --format toml or json")),
                });
            }
            Some("--post") if options.post.is_none() && action != Action::Settings => {
                options.post = Some(value.into())
            }
            Some("--mnemonic") if options.dialect.is_none() && action != Action::Settings => {
                options.dialect = Some(value.into())
            }
            Some("--settings") if options.settings.is_none() => {
                options.settings = Some(value.into())
            }
            Some("--out") if options.out.is_none() && action != Action::Watch => {
                options.out = Some(value.into())
            }
            Some("--out-dir") if options.out_dir.is_none() && data_command => {
                options.out_dir = Some(value.into())
            }
            Some("--map") if action == Action::Expand && options.map.is_none() => {
                options.map = Some(value.into())
            }
            Some("--set") => options.overrides.push(parse_set(utf8()?)?),
            Some("--block-skip-levels") if data_command => {
                let raw = utf8()?;
                let levels = if raw.is_empty() {
                    Vec::new()
                } else {
                    raw.split(',')
                        .map(|level| level.trim().parse::<u8>())
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|_| {
                            Error::config("Expected comma-separated block skip levels 1 through 9")
                        })?
                };
                options
                    .overrides
                    .push(("expand.block_skip".into(), json!(true)));
                options
                    .overrides
                    .push(("expand.block_skip_levels".into(), json!(levels)));
            }
            Some("--encoding") if data_command => {
                options.encoding = serde_json::from_value(json!(utf8()?)).map_err(Error::config)?
            }
            Some("--include") if action == Action::Run => {
                let list: Vec<_> = utf8()?.split(',').collect();
                options.include = Some(serde_json::from_value(json!(list)).map_err(Error::config)?);
            }
            Some("--watch") if action == Action::Trace => {
                for var in utf8()?.split(',') {
                    options
                        .watch
                        .push(var.trim_start_matches('#').parse().map_err(Error::config)?);
                }
            }
            Some("--error-format") if value == "json" || value == "text" => {
                options.error_json = value == "json"
            }
            _ => {
                return Err(Error::config(format!(
                    "Invalid or duplicate option: {}",
                    flag.to_string_lossy()
                )))
            }
        }
    }
    Ok(options)
}

fn configuration(action: Action, subcommand: Option<&str>, options: &Options) -> Result<u8, Error> {
    let path = match &options.inputs[..] {
        [] => None,
        [path] => Some(path.as_path()),
        _ => return Err(Error::config(USAGE)),
    };
    let config_options = options.post.is_some()
        || options.dialect.is_some()
        || options.settings.is_some()
        || !options.overrides.is_empty();
    match (action, subcommand) {
        (Action::Post, Some("show"))
            if path.is_none() && !(options.json && options.post_format.is_some()) =>
        {
            let config = options.config()?;
            let mut out = output(options.out.as_deref())?;
            if let Some(json) = options.post_format {
                let serialize = if json { post_to_json } else { post_to_string };
                writeln!(
                    out,
                    "{}",
                    serialize(&config.post).map_err(Error::config)?.trim_end()
                )?;
            } else if options.json {
                let mut value = serde_json::to_value(&config.post).map_err(Error::config)?;
                value["expand"]["optional_stop"] = json!(config.optional_stop);
                write_json(&mut out, &value)?;
            } else {
                out.write_all(
                    post_to_string(&config.post)
                        .map_err(Error::config)?
                        .as_bytes(),
                )?;
                // M01 is an application switch, outside the post format.
                writeln!(out, "\n# expand.optional_stop = {}", config.optional_stop)?;
            }
            out.flush()?;
        }
        (Action::Post, Some("init" | "check"))
            if !config_options
                && options.out.is_none()
                && !options.json
                && options.post_format.is_none() =>
        {
            let path = path.ok_or_else(|| Error::config(USAGE))?;
            if subcommand == Some("init") {
                let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
                let content = if extension.eq_ignore_ascii_case("json") {
                    post_to_json(&PostProfile::default()).map_err(Error::config)?
                } else if extension.eq_ignore_ascii_case("toml") {
                    DEFAULT_POST_TOML.into()
                } else {
                    return Err(Error::config("Expected a .toml or .json settings file"));
                };
                fs::File::create_new(path)?.write_all(content.as_bytes())?;
            } else {
                resolve_files(Some(path), None, None, [])?;
            }
        }
        (Action::Settings, Some("init" | "check" | "show")) => {
            if path.is_some() && options.settings.is_some() {
                return Err(Error::config(USAGE));
            }
            let path = path.or(options.settings.as_deref());
            let defaults = json!({"version": 1, "blockSkip": null, "blockSkipLevels": [1], "optionalStop": false});
            match subcommand {
                Some("init") if options.overrides.is_empty() => {
                    if options.out.is_some() || path.is_none() {
                        if path.is_some() {
                            return Err(Error::config(USAGE));
                        }
                        write_json(&mut output(options.out.as_deref())?, &defaults)?;
                    } else if let Some(path) = path {
                        write_json(&mut fs::File::create_new(path)?, &defaults)?;
                    }
                }
                Some("check") if options.overrides.is_empty() && options.out.is_none() => {
                    read_settings(path.ok_or_else(|| Error::config(USAGE))?)?;
                }
                Some("show") => {
                    let mut value = path
                        .map(read_settings)
                        .transpose()?
                        .unwrap_or_else(|| defaults.clone());
                    if options.overrides.iter().any(|(key, _)| {
                        !matches!(
                            key.as_str(),
                            "blockSkip"
                                | "optionalStop"
                                | "blockSkipLevels"
                                | "expand.block_skip_levels"
                                | "expand.block_skip"
                                | "expand.optional_stop"
                        )
                    }) {
                        return Err(Error::config(
                            "settings show only accepts blockSkip, blockSkipLevels and optionalStop overrides",
                        ));
                    }
                    let config = Config::resolve_settings(
                        Default::default(),
                        &value,
                        options.overrides.clone(),
                    )?;
                    value["version"] = json!(1);
                    value["blockSkip"] = json!(config.post.expand.block_skip);
                    value["blockSkipLevels"] = json!(config.post.expand.block_skip_levels);
                    value["optionalStop"] = json!(config.optional_stop);
                    write_json(&mut output(options.out.as_deref())?, &value)?;
                }
                _ => return Err(Error::config(USAGE)),
            }
        }
        _ => return Err(Error::config(USAGE)),
    }
    Ok(0)
}

fn output(path: Option<&Path>) -> Result<Box<dyn Write>, Error> {
    match path {
        Some(path) => Ok(Box::new(fs::File::create(path)?)),
        None => Ok(Box::new(io::stdout().lock())),
    }
}

struct Source {
    text: String,
    encoding: &'static str,
    filename: String,
}

fn read_source(path: &Path, encoding: Encoding) -> Result<Source, Error> {
    let bytes = if path == Path::new("-") {
        let mut bytes = Vec::new();
        io::stdin().lock().read_to_end(&mut bytes)?;
        bytes
    } else {
        fs::read(path)?
    };
    let (text, encoding) = decode(&bytes, encoding)?;
    let filename = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned();
    Ok(Source {
        text: text.into_owned(),
        encoding,
        filename,
    })
}

fn write_json(out: &mut (impl Write + ?Sized), value: &impl serde::Serialize) -> Result<(), Error> {
    serde_json::to_writer_pretty(&mut *out, value).map_err(io::Error::other)?;
    writeln!(out)?;
    Ok(())
}

fn emit_diagnostics(
    result: &nclens_core::RunResult,
    file: &str,
    json_format: bool,
) -> Result<(), Error> {
    if result.diagnostics.is_empty() && result.warnings.is_empty() {
        return Ok(());
    }
    let mut out = io::stderr().lock();
    if json_format {
        writeln!(
            out,
            "{}",
            json!({
                "code": status(result), "filename": file,
                "diagnostics": result.diagnostics, "warnings": result.warnings,
            })
        )?;
    } else {
        for diagnostic in &result.diagnostics {
            writeln!(
                out,
                "{file}:{}: {}",
                diagnostic.src_line, diagnostic.message
            )?;
        }
        for warning in &result.warnings {
            writeln!(
                out,
                "{file}:{}: warning: {}",
                warning.src_line, warning.message
            )?;
        }
    }
    Ok(())
}

fn emit_error(error: &Error, path: Option<&Path>, json_format: bool) {
    if json_format {
        let mut data = json!({"code": error.code, "message": error.message});
        if let Some(path) = path {
            data["filename"] = json!(path.to_string_lossy());
        }
        eprintln!("{data}");
    } else if let Some(path) = path {
        eprintln!("{}: {error}", path.display());
    } else {
        eprintln!("{error}");
    }
}
