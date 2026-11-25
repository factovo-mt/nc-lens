use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

const NC: &[u8] = b"%\nO1\n#1=4\nT1 M6 (10 END)\n/G0 X9\nG0 X#1\nM30\n%\n";

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_nclens-cli"))
}

fn with_input(args: &[&str], input: &[u8]) -> Output {
    let mut child = cli()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("nclens-cli-{}-{id}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn run_stdin_include_and_precedence() {
    let temp = Temp::new();
    let post = temp.0.join("machine.post.toml");
    fs::write(&post, "name = 'Machine'\n[expand]\nblock_skip = true\n").unwrap();
    let settings = temp.0.join("settings.json");
    fs::write(
        &settings,
        r#"{"version":1,"blockSkip":false,"optionalStop":true}"#,
    )
    .unwrap();
    let output = with_input(
        &[
            "run",
            "-",
            "--include",
            "expanded,trace,path",
            "--post",
            post.to_str().unwrap(),
            "--settings",
            settings.to_str().unwrap(),
            "--set",
            "codes.rapid_feed=5000",
            "--set",
            "variables.work.G54.x=10",
            "--set",
            "expand.block_skip=true",
        ],
        NC,
    );
    assert!(output.status.success(), "{output:?}");
    let data = json(&output.stdout);
    assert_eq!(data["input"]["filename"], "-");
    assert_eq!(data["input"]["encoding"], "utf-8");
    assert_eq!(data["post"]["name"], "Machine");
    assert_eq!(data["post"]["hash"].as_str().unwrap().len(), 64);
    assert!(data.get("report").is_none());
    let moves = data["expanded"].as_array().unwrap();
    assert!(moves.iter().any(|line| line["src_line"] == 6));
    assert!(!moves.iter().any(|line| line["src_line"] == 5));
    assert_eq!(data["trace"][0]["writes"][0]["value"], 4.);
    assert_eq!(data["path"][0]["to"][0], 14.);
    let output = cli()
        .args(["post", "show", "--json", "--post"])
        .arg(&post)
        .arg("--settings")
        .arg(settings)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(json(&output.stdout)["expand"]["block_skip"], false);
    assert_eq!(json(&output.stdout)["expand"]["optional_stop"], true);
}

#[test]
fn expand_encoding_out_and_exit_codes() {
    let temp = Temp::new();
    let out = temp.0.join("parsed.NCD");
    let result = with_input(&["expand", "-", "--out", out.to_str().unwrap()], NC);
    assert!(result.status.success(), "{result:?}");
    assert!(result.stdout.is_empty());
    assert!(fs::read_to_string(out).unwrap().contains("X4"));
    let bytes = b"%\n(\x8c\x8a)\nT1 M6 (10 END)\nG0 X0\nM30\n%\n";
    let result = with_input(&["run", "-"], bytes);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(json(&result.stdout)["input"]["encoding"], "cp932");

    let invalid = with_input(&["run", "-", "--error-format", "json"], b"O1\n#1=[\n");
    assert_eq!(invalid.status.code(), Some(2));
    assert_eq!(json(&invalid.stderr)["code"], 2);
    let invalid = with_input(
        &[
            "run",
            "-",
            "--set",
            "codes.rapid_feed=-1",
            "--error-format",
            "json",
        ],
        NC,
    );
    assert_eq!(invalid.status.code(), Some(3));
    assert_eq!(json(&invalid.stderr)["code"], 3);
    let invalid = cli()
        .arg("run")
        .arg(temp.0.join("missing.nc"))
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(4));

    let warning = with_input(
        &["run", "-", "--error-format", "json"],
        b"O1\nT1 M6\nG0 X10\nM30\n",
    );
    assert_eq!(warning.status.code(), Some(1), "{warning:?}");
    assert_eq!(json(&warning.stderr)["code"], 1);
}

#[test]
fn settings_lifecycle_and_batch_summary() {
    let temp = Temp::new();
    let settings = temp.0.join("settings.json");
    assert!(cli()
        .args(["settings", "init"])
        .arg(&settings)
        .status()
        .unwrap()
        .success());
    assert!(cli()
        .args(["settings", "check"])
        .arg(&settings)
        .status()
        .unwrap()
        .success());
    let shown = cli()
        .args(["settings", "show", "--set", "expand.block_skip=true"])
        .arg(&settings)
        .output()
        .unwrap();
    assert!(shown.status.success(), "{shown:?}");
    assert_eq!(json(&shown.stdout)["blockSkip"], true);

    let input = temp.0.join("source");
    let out = temp.0.join("results");
    fs::create_dir(&input).unwrap();
    fs::write(input.join("good.nc"), NC).unwrap();
    fs::write(input.join("bad.ncd"), "O1\n#1=[\n").unwrap();
    let wildcard = input.join("*.nc");
    let result = cli()
        .args(["run", "--out-dir"])
        .arg(&out)
        .arg(&input)
        .arg(wildcard)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2), "{result:?}");
    let entries = json(&result.stdout);
    assert_eq!(entries.as_array().unwrap().len(), 2);
    assert_eq!(entries[0]["status"], "failure");
    assert_eq!(entries[1]["status"], "success");
    assert!(out.join("good.report.json").is_file());
    assert!(out.join("good.expanded.NCD").is_file());

    // フォルダの一括処理：.min・大文字の .NC も対象、.txt は対象外
    let more = temp.0.join("more");
    fs::create_dir(&more).unwrap();
    fs::write(more.join("a.MIN"), NC).unwrap();
    fs::write(more.join("b.NC"), NC).unwrap();
    fs::write(more.join("notes.txt"), NC).unwrap();
    let result = cli().args(["run", "--out-dir"]).arg(&out).arg(&more).output().unwrap();
    assert!(result.status.success(), "{result:?}");
    assert_eq!(json(&result.stdout).as_array().unwrap().len(), 2);
}

struct Watch(Child);

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn watch_reprocesses_changed_nc() {
    let temp = Temp::new();
    let input = temp.0.join("work.nc");
    let out = temp.0.join("out");
    let mut watch = Watch(
        cli()
            .args(["watch"])
            .arg(&temp.0)
            .arg("--out-dir")
            .arg(&out)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut stdout = BufReader::new(watch.0.stdout.take().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        while stdout.read_line(&mut line).unwrap() != 0 {
            if tx.send(json(line.as_bytes())).is_err() {
                break;
            }
            line.clear();
        }
    });
    fs::write(&input, NC).unwrap();
    let max_wait = std::time::Duration::from_secs(8);
    assert_eq!(rx.recv_timeout(max_wait).unwrap()["status"], "success");
    assert!(fs::read_to_string(out.join("work.expanded.NCD"))
        .unwrap()
        .contains("X4"));
    fs::write(&input, "O1\nT1 M6 (10 END)\nG0 X88.\nM30\n").unwrap();
    assert_eq!(rx.recv_timeout(max_wait).unwrap()["status"], "success");
    assert!(fs::read_to_string(out.join("work.expanded.NCD"))
        .unwrap()
        .contains("X88"));
    drop(watch);
    reader.join().unwrap();
}

#[test]
fn numbered_block_skip_flags_apply_to_run_and_trace() {
    let nc = b"/G0 X1.\n/2 G0 X2.\n/3 G0 X3.\n";
    for (flags, expected) in [
        (&["--block-skip"][..], vec![2, 3]),
        (&["--block-skip-levels", "1,3"][..], vec![2]),
        (&["--block-skip-levels", ""][..], vec![1, 2, 3]),
    ] {
        let mut args = vec!["run", "-", "--include", "expanded,trace"];
        args.extend(flags);
        let output = with_input(&args, nc);
        assert_eq!(output.status.code(), Some(1), "{:?}", output);
        let result = json(&output.stdout);
        assert!(result["diagnostics"].as_array().unwrap().is_empty());
        for section in ["expanded", "trace"] {
            let lines: Vec<_> = result[section]
                .as_array()
                .unwrap()
                .iter()
                .map(|line| line["src_line"].as_u64().unwrap())
                .collect();
            assert_eq!(lines, expected, "{flags:?} {section}");
        }
    }
    assert!(!with_input(&["run", "-", "--block-skip-levels", "0,10"], nc)
        .status
        .success());
}

#[test]
fn post_show_exports_settings_reusable_by_cli() {
    let temp = Temp::new();
    let post = temp.0.join("machine.json");
    let dialect = temp.0.join("test.dialect.json");
    fs::write(&dialect, r#"{"name":"test"}"#).unwrap();
    for format in ["toml", "json"] {
        let output = cli()
            .args(["post", "show", "--format", format, "--mnemonic"])
            .arg(&dialect)
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output);
        fs::write(&post, output.stdout).unwrap();
        let check = cli().args(["post", "check"]).arg(&post).output().unwrap();
        assert!(check.status.success(), "{:?}", check);
    }
}
