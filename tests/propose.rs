use receipts::propose::split_sentences;

#[test]
fn splits_on_terminal_punctuation_followed_by_whitespace() {
    assert_eq!(
        split_sentences("First sentence. Second sentence. Third."),
        ["First sentence.", "Second sentence.", "Third."]
    );
}

#[test]
fn does_not_split_on_decimal_numbers() {
    assert_eq!(
        split_sentences("The value was 0.05 in the control group."),
        ["The value was 0.05 in the control group."]
    );
}

#[test]
fn splits_on_exclamation_and_question() {
    assert_eq!(
        split_sentences("Really? Yes! Confirmed."),
        ["Really?", "Yes!", "Confirmed."]
    );
}

#[test]
fn handles_single_sentence() {
    assert_eq!(split_sentences("Just one sentence"), ["Just one sentence"]);
}

#[test]
fn handles_empty_input() {
    assert!(split_sentences("").is_empty());
}

#[test]
fn normalizes_whitespace_within_sentences() {
    assert_eq!(
        split_sentences("Spread   out.\n Tight."),
        ["Spread out.", "Tight."]
    );
}

#[test]
fn abbreviation_with_following_capital_splits() {
    // A mis-split, and an accepted one: verification drops what does not hold.
    assert_eq!(
        split_sentences("Reported by Smith et al. Found in three cohorts."),
        ["Reported by Smith et al.", "Found in three cohorts."]
    );
}
