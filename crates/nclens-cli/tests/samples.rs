use serde_json::Value;
use std::{fs, path::Path, process::Command};

#[test]
fn paired_samples_match_cli_expansion() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut fanuc_names = Vec::new();

    for (dialect, extension) in [("fanuc", "NCD"), ("osp", "MIN")] {
        let sources = root.join("samples").join(dialect);
        let expected = root.join("tests/samples").join(dialect);
        let post = root.join(format!("samples/posts/{dialect}-sample.post.toml"));
        let mut names = Vec::new();

        for entry in fs::read_dir(sources).unwrap() {
            let source = entry.unwrap().path();
            if source.extension().and_then(|ext| ext.to_str()) != Some(extension) {
                continue;
            }

            let name = source.file_stem().unwrap().to_str().unwrap();
            let default_fixture = expected.join(format!("{name}.txt"));
            let mut cases = vec![(default_fixture, &[][..])];
            if name == "07-optional-blocks" {
                cases.push((
                    expected.join(format!("{name}-skip13.txt")),
                    &["--block-skip-levels", "1,3", "--optional-stop"][..],
                ));
            }

            for (fixture, switches) in cases {
                let output = Command::new(env!("CARGO_BIN_EXE_nclens-cli"))
                    .arg("run")
                    .arg(&source)
                    .arg("--post")
                    .arg(&post)
                    .args(["--encoding", "utf8", "--include", "expanded"])
                    .args(switches)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}: {}",
                    source.display(),
                    String::from_utf8_lossy(&output.stderr)
                );

                let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
                let expanded: Vec<_> = actual["expanded"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|line| line["text"].as_str().unwrap())
                    .collect();
                let snapshot = fs::read_to_string(&fixture).unwrap();
                assert_eq!(
                    expanded,
                    snapshot.lines().collect::<Vec<_>>(),
                    "{}",
                    fixture.display()
                );
            }

            names.push(name.to_owned());
        }

        names.sort_unstable();
        assert_eq!(names.len(), 8);
        if dialect == "fanuc" {
            fanuc_names = names;
        } else {
            assert_eq!(names, fanuc_names);
        }
    }
}
