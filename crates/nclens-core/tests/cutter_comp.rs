use nclens_core::tool::{ToolKind, ToolSource};
use nclens_core::{make_report, run, run_with, PostProfile, RunResult, ToolInfo, ToolOffset};

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
}

fn vertex(r: &RunResult, line: usize, expected: [f64; 3]) {
    let segment = r.path.iter().rfind(|s| s.src_line == line).unwrap();
    for (actual, expected) in segment.comp_to.unwrap().into_iter().zip(expected) {
        near(actual, expected);
    }
}

fn post() -> PostProfile {
    let mut post = PostProfile::default();
    post.variables.offsets.insert(
        "1".into(),
        ToolOffset {
            d_geometry: 1.25,
            d_wear: -0.25,
            ..ToolOffset::default()
        },
    );
    post
}

#[test]
fn golden_radius_comp() {
    let bytes = include_bytes!("../../../tests/golden/inputs/09_radius_comp.NCD");
    let src = String::from_utf8_lossy(bytes);
    let r = run_with(&src, &post());
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    // 左側は y=1 と x=9、次の角は y=11。右側は y=-1 と x=11、次の角は y=9。
    for (line, point) in [
        (5, [9., 1., 0.]),
        (6, [9., 11., 0.]),
        (7, [20., 11., 0.]),
        (8, [30., 10., 0.]),
        (10, [11., -1., 0.]),
        (11, [11., 9., 0.]),
        (12, [20., 9., 0.]),
        (13, [30., 10., 0.]),
    ] {
        vertex(&r, line, point);
    }
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
}

#[test]
fn rectangular_pocket_measured_on_request() {
    let src = "G17 G90\nG01 G41 D1 X10. F60\nY10.\nX0.\nY0.\nG40 Y-5.\n";
    let mut post = post();
    let programmed = run_with(src, &post);
    near(programmed.stats.cut_length_mm, 45.);
    post.codes.measure_compensated_path = true;
    let r = run_with(src, &post);
    // 1 mm 内側。立ち上がりは (0,0)→(9,1)、取消しは (1,0)→(0,-5)。
    for (line, point) in [
        (2, [9., 1., 0.]),
        (3, [9., 9., 0.]),
        (4, [1., 9., 0.]),
        (5, [1., 0., 0.]),
        (6, [0., -5., 0.]),
    ] {
        vertex(&r, line, point);
    }
    let expected = 9_f64.hypot(1.) + 8. + 8. + 9. + 5_f64.hypot(1.);
    near(r.stats.cut_length_mm, expected);
    near(r.stats.time_sec, expected); // F60 = 1 mm/s
    near(r.path.iter().map(|s| s.time_sec).sum(), expected);
    let report = make_report(&r, "pocket.nc");
    near(report.totals.cut_length_mm, expected);
    near(report.tools[&0].totals.cut_time_sec, expected);
    near(report.programs["O0000"].totals.cut_time_sec, expected);
}

#[test]
fn rectangular_exterior() {
    let src = "G90 G17\nG01 G42 D1 X10. F60\nY10.\nX0.\nY0.\nG40 Y-5.\n";
    let r = run_with(src, &post());
    for (line, point) in [
        (2, [11., -1., 0.]),
        (3, [11., 11., 0.]),
        (4, [-1., 11., 0.]),
        (5, [-1., 0., 0.]),
        (6, [0., -5., 0.]),
    ] {
        vertex(&r, line, point);
    }
}

#[test]
fn arc_with_tangent_lines_and_g10_radius() {
    let src = "T3 M6\nG90 G17 G10 L12 P7 R1.5\nG10 L13 P7 R-0.5\nG01 G41 D7 X5. F60\nG03 X10. Y5. I0. J5.\nG01 Y10.\nG40 Y15.\n";
    let r = run(src);
    // 中心 (5,5)、半径 5−(1.5−0.5)=4 の反時計回り 90 度。
    for (line, point) in [
        (4, [5., 1., 0.]),
        (5, [9., 5., 0.]),
        (6, [9., 10., 0.]),
        (7, [10., 15., 0.]),
    ] {
        vertex(&r, line, point);
    }
    for segment in r.path.iter().filter(|s| s.src_line == 5) {
        let [x, y, _] = segment.comp_to.unwrap();
        near((x - 5.).hypot(y - 5.), 4.);
    }
    assert_eq!(r.tools[0].source, ToolSource::DOffset);
    near(r.tools[0].diameter_mm, 2.);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
}

#[test]
fn other_planes_and_cancellation() {
    // G18 の面内軸は Z,X。+Z の左は +X、+X の左は −Z。
    let r = run_with("G18\nG01 G41 D1 Z10. F100\nX10.\nG40 X20.\n", &post());
    vertex(&r, 2, [1., 0., 9.]);
    vertex(&r, 3, [10., 0., 9.]);
    vertex(&r, 4, [20., 0., 10.]);
    // G19 の面内軸は Y,Z。+Y の右は −Z、+Z の右は +Y。
    let r = run_with("G19\nG01 G42 D1 Y10. F100\nZ10.\nG40 Z20.\n", &post());
    vertex(&r, 2, [0., 11., -1.]);
    vertex(&r, 3, [0., 11., 10.]);
    vertex(&r, 4, [0., 10., 20.]);
}

#[test]
fn recognizes_tools_and_reuses_indices() {
    let src = "T1 M6 (8.0END ARA H=22 L=28)\nG00 X1.\nT2 M6\n(5.0END-R-1.0-W0.5 HYPER L=20)\nX2.\n(5.0END-K45.0 SHIAGE H=3 L=20)\nT3 M6\nX3.\nT1 M6\nX4.\nT4 M6\nX5.\n";
    let mut post = post();
    post.variables.tools.insert(
        "1".into(),
        ToolInfo {
            diameter_mm: Some(9.),
            name: Some("Drill".into()),
        },
    );
    let r = run_with(src, &post);
    assert_eq!(
        r.path.iter().map(|s| s.tool_index).collect::<Vec<_>>(),
        [0, 1, 2, 0, 3]
    );
    assert_eq!(r.tools[0].source, ToolSource::Post);
    assert_eq!(r.tools[0].kind, ToolKind::Drill);
    assert_eq!(r.tools[0].diameter_mm, 9.);
    assert_eq!(r.tools[0].flute_length_mm, Some(22.));
    assert_eq!(r.tools[0].stickout_mm, Some(28.));
    assert_eq!(r.tools[1].source, ToolSource::Comment);
    assert_eq!(r.tools[1].kind, ToolKind::EndMill);
    assert_eq!(r.tools[1].corner_radius_mm, Some(1.));
    assert_eq!(r.tools[2].flute_length_mm, Some(3.));
    assert_eq!(r.tools[2].diameter_mm, 5.);
    assert_eq!(r.tools[3].source, ToolSource::Default);
    assert!(r.warnings.iter().any(|w| w.message.contains("T4")));
}

#[test]
fn warns_when_arc_cannot_be_offset() {
    let r = run_with("G01 G41 D1 X5. F60\nG03 X5.5 Y0.5 I0. J0.5\n", &post());
    assert!(r
        .warnings
        .iter()
        .any(|w| w.src_line == 2 && w.message.contains("collapses")));
    assert!(r
        .path
        .iter()
        .filter(|s| s.src_line == 2)
        .all(|s| s.comp_to.is_none()));
}

#[test]
fn trims_arc_and_line_to_their_intersection() {
    let src = "G01 G41 D1 X5. F60\nG03 X10. Y5. I0. J5.\nG01 X0.\nG40 X-5.\n";
    let r = run_with(src, &post());
    // 円 (x−5)²+(y−5)²=16 と左補正の直線 y=4 との接続点。
    let joint = [5. + 15_f64.sqrt(), 4., 0.];
    vertex(&r, 2, joint);
    let next = r.path.iter().find(|s| s.src_line == 3).unwrap();
    for (actual, expected) in next.comp_from.unwrap().into_iter().zip(joint) {
        near(actual, expected);
    }
}

#[test]
fn warns_about_undefined_d_and_limited_lookahead() {
    let r = run("G01 G41 D9 X10. F60\nY10.\nG40 Y20.\n");
    assert!(r
        .warnings
        .iter()
        .any(|w| w.src_line == 1 && w.message.contains("D number")));
    assert!(r.path.iter().all(|s| s.comp_to.is_none()));

    let r = run_with(
        "G01 G41 D1 X10. F60\nF55\nF50\nF45\nY10.\nG40 Y20.\n",
        &post(),
    );
    assert!(r
        .warnings
        .iter()
        .any(|w| w.src_line == 5 && w.message.contains("lookahead")));
}
