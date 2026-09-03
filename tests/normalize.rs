use receipts::normalize::{NormalizeOptions, normalize_with};

fn all() -> NormalizeOptions {
    NormalizeOptions::default()
}

#[test]
fn defaults_enable_every_optional_rule() {
    let options = NormalizeOptions::default();
    assert!(options.dehyphenate);
    assert!(options.quotes);
    assert_eq!(options.describe(), "whitespace, dehyphenate=on, quotes=on");
}

#[test]
fn collapses_whitespace_runs_and_trims() {
    assert_eq!(
        normalize_with("  a \t b\n\n c  ", NormalizeOptions::whitespace_only()),
        "a b c"
    );
    assert_eq!(normalize_with("a\u{00A0}\u{2003}b", all()), "a b");
    assert_eq!(normalize_with("   ", all()), "");
}

#[test]
fn rejoins_a_word_hyphenated_across_a_line_break() {
    assert_eq!(
        normalize_with("attach-\nment figure", all()),
        "attachment figure"
    );
    assert_eq!(normalize_with("differ-  \r\n  ences", all()), "differences");
    assert_eq!(normalize_with("soft\u{00AD}\nhyphen", all()), "softhyphen");
}

#[test]
fn keeps_hyphens_that_are_not_line_break_artifacts() {
    // Same line: a real hyphen followed by a space.
    assert_eq!(normalize_with("self- report", all()), "self- report");
    // Next line starts with a capital: keep the hyphen, collapse the break.
    assert_eq!(normalize_with("Main-\nHesse", all()), "Main- Hesse");
    // Next line starts with a digit.
    assert_eq!(normalize_with("2-\n5 years", all()), "2- 5 years");
    // Hyphen at the very end of the text.
    assert_eq!(normalize_with("trailing-", all()), "trailing-");
    assert_eq!(normalize_with("trailing-\n", all()), "trailing-");
}

#[test]
fn dehyphenation_can_be_disabled() {
    let options = NormalizeOptions {
        dehyphenate: false,
        quotes: true,
    };
    assert_eq!(normalize_with("attach-\nment", options), "attach- ment");
}

#[test]
fn folds_typographic_quotes_to_ascii() {
    assert_eq!(
        normalize_with(
            "people\u{2019}s \u{201C}bond\u{201D} \u{2018}x\u{2019}",
            all()
        ),
        "people's \"bond\" 'x'"
    );
    // Cambridge-style doubled single quotes fold to doubled apostrophes,
    // which is what both sides of a comparison will then contain.
    assert_eq!(
        normalize_with(
            "A \u{2018}\u{2018}rebound\u{2019}\u{2019} relationship",
            all()
        ),
        "A ''rebound'' relationship"
    );
}

#[test]
fn quote_folding_can_be_disabled() {
    let options = NormalizeOptions {
        dehyphenate: true,
        quotes: false,
    };
    assert_eq!(
        normalize_with("people\u{2019}s", options),
        "people\u{2019}s"
    );
}

#[test]
fn whitespace_only_matches_the_original_behaviour() {
    let options = NormalizeOptions::whitespace_only();
    assert_eq!(
        normalize_with("attach-\nment \u{2019}", options),
        "attach- ment \u{2019}"
    );
}
