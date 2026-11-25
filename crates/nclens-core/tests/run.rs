use nclens_core::{run, run_with_switches};

#[test]
fn author_expected_macros_preserve_vacancy_and_local_frames() {
    let source = concat!(
        "%\r\nO1\r\n",
        "#1=7\r\n#100=#0\r\nIF [#100 EQ #0] THEN #101=1\r\n",
        "G65 P2 A10. K3\r\nM98 P3 L2\r\nG1 X#1 Y#100 F100\r\nM30\r\n",
        "O2\r\n#2=ADP[#6]+ATAN[1]/[1]\r\nG1 X#1 Y#2 F100\r\nM99\r\n",
        "O3\r\n#1=#1+1\r\nM99\r%\r\n",
    );
    let r = run(source);
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    let inside = &r.expanded[1];
    assert_eq!(inside.text, "G1X10.Y48.F100");
    assert_eq!(inside.src_line, 12);
    assert_eq!(inside.call_stack, ["O0001:6"]);
    assert_eq!(r.expanded[2].text, "G1X9.F100");
    let expanded = r
        .expanded
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        run(&expanded).path.last().unwrap().to,
        r.path.last().unwrap().to
    );
    assert!(r
        .var_trace
        .iter()
        .any(|v| v.var == 100 && v.value.is_none()));
    assert!(r
        .var_trace
        .iter()
        .any(|v| v.var == 101 && v.value == Some(1.)));
}

#[test]
fn author_expected_arc_length_uses_sweep_and_helical_height() {
    let r = run("G17 G0 X10.\nG3 I-10. Z20. F100\n");
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    assert!((r.stats.cut_length_mm - (20. * std::f64::consts::PI).hypot(20.)).abs() < 1e-8);
    assert_eq!(r.path.last().unwrap().to, [10., 0., 20.]);
    assert!(r.path.len() > 60);
}

#[test]
fn author_expected_optional_switches_apply_before_execution() {
    let source = "#1=0\n/#1=1\nG1 X#1 F100\nM01\nG1 X2.\nM30\n";
    let r = run_with_switches(source, true, true);
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    assert_eq!(r.var_trace.len(), 1);
    assert!(r.path.is_empty());
    assert_eq!(r.expanded.last().unwrap().text, "M1");
}

#[test]
fn author_expected_all_programs_are_parsed_and_call_depth_is_bounded() {
    let r = run("O1\nG65 P2\nM30\nO2\nG65 P2\nM99\nO3\nG1 X[1+\n");
    assert_eq!(r.diagnostics.len(), 2, "{:?}", r.diagnostics);
    assert_eq!(r.diagnostics[0].src_line, 8);
    assert!(r.diagnostics[1].message.contains("Macro nesting limit"));
}

#[test]
fn author_expected_offsets_are_applied_to_travel_and_system_variables() {
    let r = run(concat!(
        "G10 L2 P2 X10. Y20. Z5.\n",
        "#2001=3\nG10 L11 P1 R0.5\n",
        "G55 G43 H1 G0 X0. Y0. Z20.\n",
        "#100=#5003\n#101=#5023\n#102=#10001\n",
        "G49 G0 Z20.\n",
    ));
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    assert_eq!(r.path[0].to, [10., 20., 28.5]);
    assert_eq!(r.path[1].to, [10., 20., 25.]);
    for (n, expected) in [(100, 20.), (101, 28.5), (102, 3.)] {
        assert!(r
            .var_trace
            .iter()
            .any(|v| v.var == n && v.value == Some(expected)));
    }
}

#[test]
fn segment_time_sums_to_stats() {
    // 早送り 5000 mm/min、切削は F。円弧は線分化した長さで按分
    let r = run("G90G17G00X0Y0\nG01X100.F1000\nG02X0Y0R50.\nM30\n");
    let total: f64 = r.path.iter().map(|s| s.time_sec).sum();
    assert!((total - r.stats.time_sec).abs() < 1e-9);
    assert!((r.path[0].time_sec - 6.).abs() < 1e-9, "{:?}", r.path[0]);
}

#[test]
fn segment_expanded_index_points_to_its_block() {
    // 同じ行のくり返しと、移動量 0 のブロック（線分なし）を挟む
    let r = run("G90G17G00X0Y0\n#1=0\nWHILE[#1LT3]DO1\nG01X10.F100\nX[#1*5]Y5.\n#1=#1+1\nEND1\nG02X0Y5.R10.\nM30\n");
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    for s in &r.path {
        assert_eq!(r.expanded[s.expanded_index].src_line, s.src_line, "{s:?}");
    }
    let arc: Vec<_> = r.path.iter().filter(|s| s.src_line == 8).collect();
    assert!(arc.len() > 1 && arc.iter().all(|s| s.expanded_index == arc[0].expanded_index));
    let mut idx: Vec<_> = r.path.iter().map(|s| s.expanded_index).collect();
    idx.dedup();
    assert!(idx.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn numbered_block_skip_uses_enabled_levels_and_counts_source_lines() {
    use nclens_core::{run_with_skip_levels, BlockSkipUsage};

    let source = include_str!("fixtures/block_skip_levels.nc");
    for (levels, expected) in [
        (&[][..], &[3, 4, 5, 6, 7, 9][..]),
        (&[1][..], &[4, 5, 6, 7, 9][..]),
        (&[2][..], &[3, 5, 7, 9][..]),
        (&[1, 3][..], &[4, 7, 9][..]),
        (&[1, 2, 3][..], &[7, 9][..]),
    ] {
        let result = run_with_skip_levels(source, levels, false);
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert!(result.warnings.iter().any(|warning| {
            warning.src_line == 7 && warning.message.contains("ignored trailing text")
        }));
        assert_eq!(result.var_trace[0].value, Some(4.));
        assert!(result.path.iter().all(|segment| segment.to[1] == 0.));
        let lines: Vec<_> = result.path.iter().map(|segment| segment.src_line).collect();
        assert_eq!(lines, expected, "levels {levels:?}");
        assert_eq!(
            result.block_skip_usage,
            [
                BlockSkipUsage {
                    level: 1,
                    line_count: 2
                },
                BlockSkipUsage {
                    level: 2,
                    line_count: 2
                },
                BlockSkipUsage {
                    level: 3,
                    line_count: 2
                },
            ]
        );
    }

    let legacy = run_with_switches(source, true, true);
    assert_eq!(legacy.path.last().unwrap().src_line, 7);
    assert_eq!(
        legacy
            .expanded
            .iter()
            .find(|line| line.src_line == 4)
            .unwrap()
            .text,
        "/2G0X2."
    );
    assert_eq!(
        legacy
            .expanded
            .iter()
            .find(|line| line.src_line == 6)
            .unwrap()
            .text,
        "/2/3G0X23."
    );
    assert_eq!(legacy.expanded.last().unwrap().src_line, 8);
}

#[test]
fn post_skip_levels_default_to_one_and_empty_disables_all() {
    use nclens_core::{post_from_str, run_with};

    let source = "/G0 X1.\n/2 G0 X2.\n";
    for (setting, expected) in [
        ("", vec![2]),
        ("block_skip_levels = []", vec![1, 2]),
        ("block_skip_levels = [2, 2]", vec![1]),
    ] {
        let post = post_from_str(&format!("[expand]\nblock_skip = true\n{setting}")).unwrap();
        let result = run_with(source, &post);
        let lines: Vec<_> = result.path.iter().map(|segment| segment.src_line).collect();
        assert_eq!(lines, expected, "{setting}");
    }

    for marker in ["/0", "/10", "//3"] {
        let result = run(&format!("{marker} G0 X1."));
        assert_eq!(result.diagnostics.len(), 1, "{marker}");
        assert!(result.path.is_empty());
    }
    assert!(post_from_str("[expand]\nblock_skip_levels = [10]").is_err());
}
