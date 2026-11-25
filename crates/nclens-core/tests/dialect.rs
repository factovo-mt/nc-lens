use std::fs;

use nclens_core::{
    builtin_dialects, load_dialect, load_post, make_report, run_with, save_dialect, save_post,
    validate_dialect, AddressRole, CustomCode, GCodeDefinition, IntegerUnits, MCodeDefinition,
    PostProfile,
};

#[test]
fn dialect_and_post_round_trip() {
    let mut post = custom_post();
    let dir = std::env::temp_dir();
    let dialect_path = dir.join(format!(
        "nclens-dialect-{}.dialect.toml",
        std::process::id()
    ));
    let post_path = dialect_path.with_extension("post.toml");
    for dialect in builtin_dialects() {
        assert!(validate_dialect(&dialect).is_empty());
        save_dialect(&dialect_path, &dialect).unwrap();
        assert_eq!(load_dialect(&dialect_path).unwrap(), dialect);
    }
    save_dialect(&dialect_path, &post.dialect).unwrap();
    assert_eq!(load_dialect(&dialect_path).unwrap(), post.dialect);
    save_post(&post_path, &post).unwrap();
    assert_eq!(load_post(&post_path).unwrap(), post);

    post.dialect.g_codes.get_mut("100.1").unwrap().function = "typo".into();
    assert_eq!(validate_dialect(&post.dialect).len(), 1);
    assert!(save_dialect(&dialect_path, &post.dialect).is_err());
    fs::write(
        &dialect_path,
        "name = \"bad\"\n\n[g_codes]\n\"1\" = { function = \"typo\" }\n",
    )
    .unwrap();
    assert_eq!(load_dialect(&dialect_path).unwrap_err().line, Some(4));
    fs::remove_file(&dialect_path).unwrap();
    fs::remove_file(post_path).unwrap();
}

fn custom_post() -> PostProfile {
    let mut post = PostProfile::default();
    post.dialect.name = "custom".into();
    post.dialect.base = Some("fanuc".into());
    post.dialect.addresses.extend([
        ('U', AddressRole::X),
        ('Q', AddressRole::A),
        ('L', AddressRole::O),
        ('J', AddressRole::N),
    ]);
    post.dialect.g_codes.insert(
        "100.1".into(),
        GCodeDefinition {
            function: "linear".into(),
            group: Some(1),
            description: String::new(),
        },
    );
    post.dialect.g_codes.insert(
        "43.1".into(),
        GCodeDefinition {
            function: "length_dynamic".into(),
            group: Some(8),
            description: String::new(),
        },
    );
    post.dialect.m_codes.insert(
        "70".into(),
        MCodeDefinition {
            function: "call".into(),
            program: Some(11),
            description: String::new(),
        },
    );
    post.dialect
        .custom
        .insert("G100".into(), CustomCode::Call(12));
    post.dialect
        .custom
        .insert("W20".into(), CustomCode::Replace("G100.1 U40. F120".into()));
    post.codes.integer_units = IntegerUnits::Millimeters;
    post
}

#[test]
fn dialect_dispatches_remapped_addresses_codes_and_user_rules() {
    let post = custom_post();
    let result = run_with(
        "L1\nG90 G100.1 U10. Q90. F120\nG43.1 Z2.\nG91 Q-20.\nM70\nG100 A5.\nW20\nM2\nL11\nJ1 G91 G1 U3.\nM99\nL12\nG91 G1 U#1\nM99\n",
        &post,
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(result.expanded[1].modal.g_codes.get(&1), Some(&100.1));
    assert_eq!(
        result.expanded[1].modal.position,
        [10., 0., 0., 90., 0., 0.]
    );
    assert_eq!(result.expanded[2].modal.g_codes.get(&8), Some(&43.1));
    assert_eq!(result.expanded[2].modal.position[2], -2.);
    assert_eq!(result.expanded[3].modal.position[3], 70.);
    assert_eq!(result.bounds.work.max[3], 90.);
    assert_eq!(result.path.last().unwrap().to[0], 58.);
    assert!(result
        .expanded
        .iter()
        .any(|line| line.text.contains("G100.1U40")));
    assert_eq!(result.expanded[0].text, "L0001");
}

#[test]
fn report_measures_offsets_moves_dwell_and_tools() {
    let mut post = PostProfile::default();
    post.variables.work[0] = [100., 0., 0.];
    post.codes.rapid_mm_min = 6000.;
    post.codes.atc_sec = 2.;
    post.name = "measurement".into();
    let result = run_with(
        "O7\nT5 M6\nG54 G90 G0 X0.\nG1 X10. A45. F120\nG4 P500\nT6 M6\nG91 G1 X4. A-10. F60\nM30\n",
        &post,
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let report = make_report(&result, "job.nc");
    assert_eq!(report.filename, "job.nc");
    assert!(report.generated_at_unix_seconds > 0);
    assert_eq!(report.dialect, "fanuc");
    assert_eq!(report.post_name, "measurement");
    assert_eq!(report.totals.rapid_length_mm, 100.);
    assert_eq!(report.totals.cut_length_mm, 14.);
    assert_eq!(report.totals.rapid_time_sec, 1.);
    assert_eq!(report.totals.cut_time_sec, 9.);
    assert_eq!(report.totals.other_time_sec, 4.5);
    assert_eq!((report.feed_min, report.feed_max), (Some(60.), Some(120.)));
    assert_eq!(report.bounds.machine.max[0], 114.);
    assert_eq!(report.bounds.work.min[0], -100.);
    assert_eq!(report.bounds.work.max[0], 14.);
    assert_eq!(report.bounds.work.max[3], 45.);
    assert_eq!(report.tools[&5].totals.cut_length_mm, 10.);
    assert_eq!(report.tools[&6].totals.cut_length_mm, 4.);
    assert_eq!(
        report.tools[&6]
            .used_lines
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [6, 7, 8]
    );
    assert_eq!(report.programs["O0007"].totals, report.totals);
    assert_eq!(report.tools[&6].bounds, result.tool_bounds[&6]);
    assert_eq!((report.errors, report.warnings), (0, 0));
}

#[test]
fn expanded_cycle_rows_have_each_intermediate_position() {
    let mut post = PostProfile::default();
    post.expand.cycles = true;
    let result = run_with("G90 G0 Z10.\nG81 X5. Z-4. R2. F100\nG80\nM30", &post);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    for seg in &result.path {
        let row = &result.expanded[seg.expanded_index];
        assert_eq!(&row.modal.position[..3], &seg.to);
    }
    assert!(result.path.len() >= 4);
}

#[test]
fn osp_programs_match_author_expected_positions() {
    let mut post = PostProfile::default();
    post.dialect.name = "osp".into();
    post.variables.work[1] = [100., 0., 0.];
    for (src, expected, axes) in [
        (include_str!("osp/flow.nc"), [6., 0., 0.], "OMAIN1"),
        (include_str!("osp/call.nc"), [6., 10., 0.], "OMAIN2"),
        (include_str!("osp/modal.nc"), [3., 4., 0.], "OMAIN3"),
        (include_str!("osp/work.nc"), [105., 0., 0.], "OMAIN4"),
    ] {
        let result = run_with(src, &post);
        assert!(
            result.diagnostics.is_empty(),
            "{axes} {:?}",
            result.diagnostics
        );
        assert_eq!(result.expanded[0].text, axes);
        assert_eq!(result.path.last().unwrap().to, expected, "{axes}");
        let report = make_report(&result, "osp.nc");
        assert_eq!(report.dialect, "osp");
        assert!(report.programs.contains_key(axes));
        if axes == "OMAIN4" {
            assert_eq!(report.totals.other_time_sec, 2.5);
            assert_eq!(report.feed_max, Some(120.));
            assert_eq!(result.expanded[1].modal.work_coordinate, 15.);
            assert_eq!(result.expanded[1].modal.g_codes.get(&4), Some(&15.));
        }
    }
}

#[test]
fn report_counts_parse_errors_and_cutting_blocks_without_a_time_estimate() {
    let result = run_with(
        "O1\nG1 X1.\nG1 X2. F60\n#500=\nM30",
        &PostProfile::default(),
    );
    let report = make_report(&result, "unmetered.nc");
    assert_eq!((report.errors, report.warnings), (1, 1));
    assert_eq!(report.totals.cut_length_mm, 2.);
    assert_eq!(report.totals.cut_time_sec, 1.);
}
