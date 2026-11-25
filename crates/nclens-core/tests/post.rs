use nclens_core::{
    load_post, run_through_stops, run_with, save_post, CompensationType, DistanceMode,
    IntegerUnits, PostProfile, StopKind, ToolChange, ToolInfo, ToolOffset,
};
use std::{fs, path::Path};

#[test]
fn preview_records_stops_without_ending_the_program() {
    let mut post = PostProfile::default();
    for end in ["M02", "M30"] {
        let src = format!("G00 X1.\nM01\n/M00\nG01 X2. F60\nM00\nG01 X3.\n{end}\nG00 X4.");
        let legacy = run_with(&src, &post);
        assert!(legacy.stops.is_empty());
        assert_eq!(legacy.path.last().unwrap().to[0], 1.);

        let preview = run_through_stops(&src, &post);
        assert!(preview.diagnostics.is_empty(), "{:?}", preview.diagnostics);
        assert_eq!(preview.path.last().unwrap().to[0], 3.);
        assert_eq!(
            preview
                .stops
                .iter()
                .map(|s| (s.kind, s.src_line))
                .collect::<Vec<_>>(),
            [(StopKind::M01, 2), (StopKind::M00, 3), (StopKind::M00, 5)]
        );
        for stop in preview.stops {
            assert_eq!(
                preview.expanded[stop.expanded_index].src_line,
                stop.src_line
            );
        }
    }
    post.expand.block_skip = true;
    let preview = run_through_stops("G00 X1.\n/M00\nM01\nG00 X2.\nM30", &post);
    assert_eq!(preview.stops.len(), 1);
    assert_eq!(preview.stops[0].kind, StopKind::M01);
    assert_eq!(preview.path.last().unwrap().to[0], 2.);
}

#[test]
fn preview_preserves_cycle_modes_and_links_calls_without_extra_output() {
    let src = "O1\nM98 P2\nM30\nO2\nG81 X1. Z-2. R1. F60\nX2.\nG80\nM99";
    let mut post = PostProfile::default();
    post.expand.cycles = true;
    let result = run_through_stops(src, &post);
    let legacy = run_with(src, &post);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert!(result
        .expanded
        .iter()
        .map(|r| &r.text)
        .eq(legacy.expanded.iter().map(|r| &r.text)));
    let [call, ret] = result.calls.as_slice() else {
        panic!("expected a call and return")
    };
    assert_eq!((call.src_line, ret.src_line), (2, 8));
    assert_eq!(result.expanded[call.expanded_index_after].src_line, 5);
    assert_eq!(result.expanded[ret.expanded_index_after - 1].src_line, 7);
    for segment in &result.path {
        let line = &result.expanded[segment.expanded_index];
        assert!(line
            .source_g_codes
            .as_ref()
            .unwrap()
            .values()
            .any(|&v| v == 81.));
    }
}

#[test]
fn post_file_round_trip_and_validation() {
    let default = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/default.post.toml");
    let mut post = load_post(default).unwrap();
    assert_eq!(post, PostProfile::default());
    post.name = "試作ポスト".into();
    post.comment = "1行目\n2行目".into();
    post.codes.g_calls.insert("81.1".into(), 9010);
    post.variables.common.insert("500".into(), 1.25);
    post.variables.work[5] = [12., -1.75, 5.];
    post.variables.offsets.insert(
        "999".into(),
        ToolOffset {
            h_geometry: 4.,
            h_wear: 0.125,
            d_geometry: 2.5,
            d_wear: -0.125,
        },
    );
    post.variables.tools.insert(
        "999".into(),
        ToolInfo {
            diameter_mm: Some(6.),
            name: Some("ドリル".into()),
        },
    );
    post.expand.distance_mode = DistanceMode::Incremental;
    let path =
        std::env::temp_dir().join(format!("nclens-roundtrip-{}.post.toml", std::process::id()));
    save_post(&path, &PostProfile::default()).unwrap();
    save_post(&path, &post).unwrap();
    let loaded = load_post(&path);
    fs::write(&path, "version = 1\n\n[codes]\nrapid_mm_min = -1.0\n").unwrap();
    let invalid_value = load_post(&path);
    fs::write(&path, "version = 1\n\n[codes]\nrapid_mm_min = ?\n").unwrap();
    let invalid_toml = load_post(&path);
    fs::write(&path, "[codes.g_calls]\n\"81.1\" = 9010\n\"81.2\" = 9001\n").unwrap();
    let invalid_call = load_post(&path);
    fs::remove_file(&path).unwrap();
    assert_eq!(loaded.unwrap(), post);
    assert_eq!(invalid_value.unwrap_err().line, Some(4));
    assert_eq!(invalid_toml.unwrap_err().line, Some(4));
    assert_eq!(invalid_call.unwrap_err().line, Some(3));
}

#[test]
fn profile_initial_values_and_system_variables_share_state() {
    let mut post = PostProfile::default();
    post.variables.external = [2., 0., 0.];
    post.variables.work[0] = [5., 0., 0.];
    post.variables.common.insert("100".into(), 3.);
    post.variables.offsets.insert(
        "1".into(),
        ToolOffset {
            h_geometry: 10.,
            h_wear: 0.5,
            d_geometry: 2.,
            d_wear: 0.25,
        },
    );
    let result = run_with(
        "#500=#100+#5201+#5221+#2001+#2201+#2401+#2601\n\
         #10001=3.\n#5221=8.\nG54 G43 H1 G0 X0. Z0.\n#501=#2001\nM30",
        &post,
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(result.var_trace[0].value, Some(22.75));
    assert_eq!(result.var_trace.last().unwrap().value, Some(3.));
    assert_eq!(result.path.last().unwrap().to, [10., 0., 3.5]);
    assert_eq!(post.variables.work[0][0], 5.);
}

#[test]
fn configured_calls_and_switches_take_effect() {
    let mut post = PostProfile::default();
    post.codes.program_address = ':';
    post.codes.integer_units = IntegerUnits::Millimeters;
    post.codes.m_calls.insert("80".into(), 9001);
    post.codes.g_calls.insert("81.1".into(), 9010);
    post.codes.t_call = true;
    post.codes.ignore_m.push(0);
    post.expand.block_skip = true;
    let result = run_with(
        ":1\n/M30\nM0\nM80\nG81.1 X10\nT7\nM30\n\
         :9001\n#100=3\nM99\n:9010\nG0X#24\nM99\n:9000\n#101=#149\nM99",
        &post,
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(result.expanded.first().unwrap().text, ":0001");
    assert_eq!(result.path.last().unwrap().to[0], 10.);
    assert_eq!(
        result
            .var_trace
            .iter()
            .map(|v| (v.var, v.value))
            .collect::<Vec<_>>(),
        [
            (100, Some(3.)),
            (24, Some(10.)),
            (149, Some(7.)),
            (101, Some(7.))
        ]
    );
}

#[test]
fn configured_expansion_preserves_path() {
    let mut post = PostProfile::default();
    post.expand.cycles = true;
    post.expand.distance_mode = DistanceMode::Incremental;
    post.expand.omit_zero_moves = true;
    post.expand.decimal_point = false;
    post.expand.g10 = true;
    let result = run_with(
        "G90G0X10.Y0.Z10.\nG91G10L2P1X2.\nG90G83X10.Z-4.R2.Q2.F60\nG80\nG0X15.Z8.\nM30",
        &post,
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let expanded = result
        .expanded
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!expanded.contains("G83"), "{expanded}");
    assert!(!expanded.contains("Y0"), "{expanded}");
    assert!(expanded.contains("G90G10L2P1X2000"), "{expanded}");
    let replay = run_with(&expanded, &PostProfile::default());
    assert!(replay.diagnostics.is_empty(), "{:?}", replay.diagnostics);
    assert_eq!(
        replay.path.iter().map(|s| s.to).collect::<Vec<_>>(),
        result.path.iter().map(|s| s.to).collect::<Vec<_>>(),
        "{expanded}"
    );
}

#[test]
fn reference_points_and_code_timing_use_profile() {
    let mut post = PostProfile::default();
    post.codes.rapid_mm_min = 600.;
    post.codes.max_rpm = 500.;
    post.codes.tool_change = ToolChange::T;
    post.codes.atc_sec = 2.;
    post.variables.g28 = [20., 0., 0.];
    post.variables.g30[2] = [30., 0., 0.];
    let result = run_with(
        "T1\nM6\nT1\nT2\nG91G28X0.\nG30P4X0.\nG90G97G95S1000\nG1X40.F0.2\nM30",
        &post,
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert!((result.stats.time_sec - 13.).abs() < 1e-9);
    assert_eq!(
        result.path.iter().map(|s| s.to[0]).collect::<Vec<_>>(),
        [20., 30., 40.]
    );
    assert!(result.path.iter().all(|s| s.tool_index == 1));
}

#[test]
fn compensation_type_changes_g10_layout() {
    let mut post = PostProfile::default();
    post.codes.compensation = CompensationType::B;
    let result = run_with(
        "G10L10P1R2.\nG10L11P1R3.\n#500=#2001+#2201\n#2201=5.\n#501=#12001\nG43H1G0Z0.\nM30",
        &post,
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(
        result
            .var_trace
            .iter()
            .find(|v| v.var == 500)
            .unwrap()
            .value,
        Some(5.)
    );
    assert_eq!(
        result
            .var_trace
            .iter()
            .find(|v| v.var == 501)
            .unwrap()
            .value,
        Some(5.)
    );
    assert_eq!(result.path.last().unwrap().to[2], 2.);
}

#[test]
fn json_settings_round_trip_and_content_is_detected() {
    use nclens_core::{
        dialect_from_json, dialect_to_json, load_dialect, post_from_json, post_to_json,
        save_dialect, DuplicateWord,
    };

    let mut post = PostProfile::default();
    post.codes.duplicate_word = DuplicateWord::First;
    let encoded = post_to_json(&post).unwrap();
    assert_eq!(post_from_json(&encoded).unwrap(), post);
    assert!(post_from_json(r#"{"codes":{"duplicate_word":"invalid"}}"#).is_err());
    assert_eq!(post_from_json("{}").unwrap(), PostProfile::default());

    let base = std::env::temp_dir().join(format!("nclens-json-roundtrip-{}", std::process::id()));
    let json_path = base.with_extension("json");
    let toml_path = base.with_extension("toml");
    save_post(&json_path, &post).unwrap();
    assert_eq!(load_post(&json_path).unwrap(), post);
    std::fs::rename(&json_path, &toml_path).unwrap();
    assert_eq!(load_post(&toml_path).unwrap(), post);
    std::fs::remove_file(toml_path).unwrap();

    let dialect = post.dialect;
    assert_eq!(
        dialect_from_json(&dialect_to_json(&dialect).unwrap()).unwrap(),
        dialect
    );
    save_dialect(&json_path, &dialect).unwrap();
    assert_eq!(load_dialect(&json_path).unwrap(), dialect);
    std::fs::remove_file(json_path).unwrap();
}
