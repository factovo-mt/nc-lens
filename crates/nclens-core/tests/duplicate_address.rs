use nclens_core::{run_with, DuplicateM, DuplicateWord, PostProfile, SegKind};

const NC: &str = include_str!("fixtures/duplicate-address.nc");

#[test]
fn default_selects_last_words_and_all_m_codes() {
    let result = run_with(NC, &PostProfile::default());
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(result.path[0].to[0], 20.);
    assert_eq!(result.path[1].kind, SegKind::Cut);
    assert!(result.expanded.iter().any(|line| line.text == "M3M8"));
    assert!(result
        .expanded
        .iter()
        .all(|line| !line.text.contains("X10")));
    for (var, value) in [(100, 8.), (101, 1.), (102, 5.)] {
        assert!(result
            .var_trace
            .iter()
            .any(|change| change.var == var && change.value == Some(value)));
    }
    for (line, word) in [(3, "X"), (4, "G group"), (5, "M"), (6, "macro argument A")] {
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.src_line == line && w.message.contains(word)),
            "{:?}",
            result.warnings
        );
    }
}

#[test]
fn first_word_and_modal_group_are_used_with_warnings_disabled() {
    let mut post = PostProfile::default();
    post.codes.duplicate_word = DuplicateWord::First;
    post.codes.duplicate_g_group = DuplicateWord::First;
    post.codes.duplicate_m = DuplicateM::First;
    post.codes.warn_on_duplicate = false;
    let result = run_with(NC, &post);
    assert!(result.diagnostics.is_empty());
    assert!(result
        .warnings
        .iter()
        .all(|w| w.message == "Unknown diameter for T0"));
    assert_eq!(result.path[0].to[0], 10.);
    assert_eq!(result.path[1].kind, SegKind::Rapid);
    assert!(result.expanded.iter().any(|line| line.text == "M3"));
    assert!(result.expanded.iter().all(|line| !line.text.contains("M8")));
    assert!(result
        .var_trace
        .iter()
        .any(|change| change.var == 100 && change.value == Some(4.)));
}

#[test]
fn errors_skip_the_block_and_m_limit_warns_even_if_duplicate_warnings_are_disabled() {
    let mut post = PostProfile::default();
    post.codes.duplicate_word = DuplicateWord::Error;
    post.codes.duplicate_g_group = DuplicateWord::Error;
    post.codes.duplicate_m = DuplicateM::Last;
    post.codes.max_m_per_block = 1;
    post.codes.warn_on_duplicate = false;
    let result = run_with(NC, &post);
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.src_line)
            .collect::<Vec<_>>(),
        [3, 4, 6]
    );
    assert!(result.path.is_empty());
    assert!(result.expanded.iter().any(|line| line.text == "M8"));
    assert!(result.expanded.iter().all(|line| line.text != "M3"));
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.warnings[0].src_line, 5);
    assert!(result.warnings[0].message.contains("kept [8.0]"));
}
