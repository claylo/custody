use receipts::pdf::mutool::parse_stext_json;

#[test]
fn parses_structured_text_in_block_line_order() {
    let pages =
        parse_stext_json(include_str!("fixtures/mutool-prose.json")).expect("fixture parses");

    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].page, 1);
    assert!(
        pages[0]
            .text
            .contains("transport in this regime is the absence of measurable turbulent mixing")
    );
}

#[test]
fn leaves_fragmented_pdf_tokens_unrepaired() {
    let pages = parse_stext_json(include_str!("fixtures/mutool-rotated-table.json"))
        .expect("fixture parses");

    assert!(!pages[0].text.contains("42.8% within tolerance"));
    assert!(pages[0].text.contains("42 . 8 % within tolera nce"));
}

#[test]
fn rejects_output_without_pages() {
    let error = parse_stext_json(r#"{"file":"empty.pdf"}"#).expect_err("pages are required");

    assert!(error.to_string().contains("pages"));
}
