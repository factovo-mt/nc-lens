use std::path::Path;
use std::process::Command;

use serde_json::Value;

#[test]
fn detailed_json_report_selects_osp_and_keeps_legacy_output() {
    let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("../nclens-core/tests/osp/flow.nc");
    let output = Command::new(env!("CARGO_BIN_EXE_nclens-cli"))
        .args(["report", "--json", "--dialect", "osp"])
        .arg(&input)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["filename"], "flow.nc");
    assert_eq!(report["dialect"], "osp");
    assert_eq!(report["errors"], 0);
    assert_eq!(report["bounds"]["work"]["max"][0], 6.);
    assert_eq!(report["programs"]["OMAIN1"]["totals"]["cut_length_mm"], 6.);

    let output = Command::new(env!("CARGO_BIN_EXE_nclens-cli"))
        .args(["report", "--dialect", "osp"])
        .arg(input)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let legacy: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(legacy["cut_length_mm"], 6.);
    assert!(legacy.get("totals").is_none());
}
