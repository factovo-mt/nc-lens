use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, SystemTime},
};

use nclens_cli::{execute, expanded_nc, run_output, status, wants_trace, Config, Error, Include};
use nclens_core::make_report;
use serde::Serialize;

use crate::{emit_diagnostics, emit_error, read_source, write_json, Action, Options};

#[derive(Serialize)]
struct Entry {
    input: PathBuf,
    status: &'static str,
    code: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

fn entry(input: &Path, result: Result<u8, Error>, error_json: bool) -> Entry {
    let (code, message) = match result {
        Ok(code) => (code, None),
        Err(error) => {
            emit_error(&error, Some(input), error_json);
            (error.code, Some(error.message))
        }
    };
    Entry {
        input: input.into(),
        status: match code {
            0 => "success",
            1 => "warning",
            _ => "failure",
        },
        code,
        message,
    }
}

pub(super) fn run(
    action: Action,
    dir: &Path,
    options: &Options,
    config: &Config,
) -> Result<u8, Error> {
    let mut paths = BTreeSet::new();
    for pattern in &options.inputs {
        if pattern == Path::new("-") {
            return Err(Error::config("Batch input requires a path"));
        }
        if pattern.is_dir() {
            paths.extend(scan(pattern)?.into_keys());
        } else if pattern.exists() {
            paths.insert(pattern.canonicalize()?);
        } else if let Some(pattern) = pattern.to_str().filter(|s| s.contains(['*', '?', '['])) {
            let mut matched = false;
            for path in glob::glob(pattern).map_err(Error::config)? {
                let path = path.map_err(io::Error::other)?;
                if path.is_file() {
                    paths.insert(path.canonicalize()?);
                    matched = true;
                }
            }
            if !matched {
                paths.insert(PathBuf::from(pattern));
            }
        } else {
            paths.insert(pattern.clone());
        }
    }

    fs::create_dir_all(dir)?;
    let dir = dir.canonicalize()?;
    let collisions = collisions(&paths);
    let paths: Vec<_> = paths.into_iter().collect();
    let workers = thread::available_parallelism()
        .map_or(1, usize::from)
        .min(paths.len());
    let next = AtomicUsize::new(0);
    let entries = Mutex::new(Vec::with_capacity(paths.len()));
    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(path) = paths.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let result = if collisions.contains(&stem(path)) {
                        Err(Error::config("Multiple inputs have the same output name"))
                    } else {
                        process(action, path, &dir, options, config)
                    };
                    let item = entry(path, result, options.error_json);
                    entries.lock().unwrap().push(item);
                }
            });
        }
    });
    let mut entries = entries.into_inner().unwrap();
    entries.sort_by(|a, b| a.input.cmp(&b.input));
    write_json(&mut io::stdout().lock(), &entries)?;
    Ok(entries.iter().map(|e| e.code).max().unwrap_or(0))
}

type Stamp = (SystemTime, u64);

fn scan(dir: &Path) -> io::Result<BTreeMap<PathBuf, Stamp>> {
    let mut files = BTreeMap::new();
    for item in fs::read_dir(dir)? {
        let item = item?;
        let name = item.file_name().to_string_lossy().to_ascii_lowercase();
        let path = item.path();
        if name.ends_with(".expanded.ncd")
            || name.ends_with(".expanded.nc")
            || !path
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|ext| {
                    ["nc", "min", "ncd", "dnc", "tap", "cnc", "mpf", "spf", "eia"]
                        .iter()
                        .any(|x| ext.eq_ignore_ascii_case(x))
                })
            || !item.file_type()?.is_file()
        {
            continue;
        }
        let metadata = item.metadata()?;
        files.insert(path.canonicalize()?, (metadata.modified()?, metadata.len()));
    }
    Ok(files)
}

pub(super) fn watch(
    input: &Path,
    dir: &Path,
    options: &Options,
    config: &Config,
) -> Result<u8, Error> {
    let input = input.canonicalize()?;
    if !input.is_dir() {
        return Err(Error::config("Watch input must be a directory"));
    }
    fs::create_dir_all(dir)?;
    let dir = dir.canonicalize()?;
    let mut previous = BTreeMap::new();
    let mut done = BTreeMap::<PathBuf, Stamp>::new();
    let mut reported_collisions = BTreeSet::<PathBuf>::new();
    loop {
        let current = match scan(&input) {
            Ok(current) => current,
            Err(err) if err.kind() == io::ErrorKind::NotFound && input.is_dir() => {
                thread::sleep(Duration::from_millis(500));
                continue;
            }
            Err(err) => return Err(err.into()),
        };
        let conflicts = collisions(current.keys());
        reported_collisions
            .retain(|path| current.contains_key(path) && conflicts.contains(&stem(path)));
        done.retain(|path, _| current.contains_key(path) && !conflicts.contains(&stem(path)));
        for (path, stamp) in &current {
            if previous.get(path) != Some(stamp) || done.get(path) == Some(stamp) {
                continue;
            }
            let result = if conflicts.contains(&stem(path)) {
                if !reported_collisions.insert(path.clone()) {
                    continue;
                }
                Err(Error::config("Multiple inputs have the same output name"))
            } else {
                process(Action::Run, path, &dir, options, config)
            };
            // Retry transient read/write errors on the next stable poll.
            if !matches!(&result, Err(err) if err.code == 4) {
                done.insert(path.clone(), *stamp);
            }
            let line = serde_json::to_string(&entry(path, result, options.error_json))
                .map_err(io::Error::other)?;
            let mut out = io::stdout().lock();
            writeln!(out, "{line}")?;
            out.flush()?;
        }
        previous = current;
        thread::sleep(Duration::from_millis(500));
    }
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .to_lowercase()
}

fn collisions<'a>(paths: impl IntoIterator<Item = &'a PathBuf>) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut collisions = BTreeSet::new();
    for path in paths {
        let name = stem(path);
        if !seen.insert(name.clone()) {
            collisions.insert(name);
        }
    }
    collisions
}

fn process(
    action: Action,
    path: &Path,
    dir: &Path,
    options: &Options,
    config: &Config,
) -> Result<u8, Error> {
    let source = read_source(path, options.encoding)?;
    let include = options.include();
    let combined = action == Action::Run && options.include.is_some();
    let (result, trace) = execute(&source.text, config, combined && wants_trace(include));
    emit_diagnostics(&result, &source.filename, options.error_json)?;

    let output_path = |suffix: &str| {
        let mut name = path.file_stem().unwrap_or(path.as_os_str()).to_os_string();
        name.push(suffix);
        dir.join(name)
    };

    if action == Action::Report
        || (action == Action::Run && include.iter().any(|x| matches!(x, Include::Report)))
    {
        write_atomic(&output_path(".report.json"), |file| {
            write_json(file, &make_report(&result, &source.filename))
        })?;
    }
    if action == Action::Expand
        || (action == Action::Run && include.iter().any(|x| matches!(x, Include::Expanded)))
    {
        write_atomic(&output_path(".expanded.NCD"), |file| {
            file.write_all(expanded_nc(&result).as_bytes())?;
            Ok(())
        })?;
    }
    if combined {
        let output = run_output(
            &result,
            trace,
            config,
            &source.filename,
            source.encoding,
            include,
        );
        write_atomic(&output_path(".run.json"), |file| write_json(file, &output))?;
    }
    Ok(status(&result))
}

pub(super) fn write_atomic(
    path: &Path,
    write: impl FnOnce(&mut fs::File) -> Result<(), Error>,
) -> Result<(), Error> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let (tmp, mut file) = loop {
        let seq = NEXT.fetch_add(1, Ordering::Relaxed);
        let tmp = path.with_file_name(format!(".nclens-{}-{seq}.tmp", std::process::id()));
        match fs::File::create_new(&tmp) {
            Ok(file) => break (tmp, file),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.into()),
        }
    };
    let result = write(&mut file);
    drop(file);
    let result = result.and_then(|()| fs::rename(&tmp, path).map_err(Error::from));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}
