use receipts::pdf::{
    matching_bbox,
    mutool::{parse_stext_json, validate_version},
};

#[test]
fn validates_the_supported_mutool_version_line() {
    assert!(validate_version("mutool version 1.28.0").is_ok());
    assert!(validate_version("mutool version 1.28.99").is_ok());
    assert!(validate_version("mutool version 1.27.9").is_err());
    assert!(validate_version("mutool version 1.29.0").is_err());
    assert!(validate_version("mutool unknown").is_err());
}

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
    let bbox = matching_bbox(
        &pages[0],
        "transport in this regime is the absence of measurable turbulent mixing",
    )
    .unwrap();
    assert_eq!(
        (bbox.x, bbox.y, bbox.width, bbox.height),
        (10.0, 20.0, 300.0, 24.0)
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

#[test]
fn rejects_pages_without_blocks() {
    let error =
        parse_stext_json(r#"{"pages":[{"groups":[]}]}"#).expect_err("the blocks field is required");

    assert!(error.to_string().contains("blocks"));
}

#[test]
fn rejects_blocks_without_lines() {
    let error = parse_stext_json(r#"{"pages":[{"blocks":[{"type":"text","spans":[]}]}]}"#)
        .expect_err("the lines field is required");

    assert!(error.to_string().contains("lines"));
}

#[test]
fn preserves_non_text_blocks_for_ocr_fallback() {
    let pages = parse_stext_json(r#"{"pages":[{"blocks":[{"type":"image"}]}]}"#)
        .expect("image-only pages remain valid native extraction results");

    assert!(pages[0].text.is_empty());
    assert!(pages[0].spans.is_empty());
}

#[test]
fn preserves_lines_without_geometry() {
    let pages =
        parse_stext_json(r#"{"pages":[{"blocks":[{"type":"text","lines":[{"text":"plain"}]}]}]}"#)
            .expect("bbox remains optional");

    assert_eq!(pages[0].text, "plain");
    assert_eq!(pages[0].spans[0].bbox, None);
}

#[test]
fn rejoins_words_hyphenated_across_stext_lines() {
    let pages = parse_stext_json(
        r#"{"pages":[{"blocks":[{"type":"text","lines":[
            {"text":"The infant showed an attach-","bbox":{"x":10,"y":20,"w":200,"h":12}},
            {"text":"ment behaviour toward the mother.","bbox":{"x":10,"y":34,"w":220,"h":12}}
        ]}]}]}"#,
    )
    .expect("fixture parses");

    assert_eq!(
        pages[0].text,
        "The infant showed an attachment behaviour toward the mother."
    );
    // The match crosses the rejoined hyphen; both lines' boxes are unioned.
    let bbox = matching_bbox(&pages[0], "an attachment behaviour").unwrap();
    assert_eq!(
        (bbox.x, bbox.y, bbox.width, bbox.height),
        (10.0, 20.0, 220.0, 26.0)
    );
}

#[test]
fn parses_blocks_as_normalized_paragraphs() {
    use receipts::pdf::mutool::parse_stext_blocks;

    let pages = parse_stext_blocks(
        r#"{"pages":[
            {"blocks":[
                {"type":"text","lines":[{"text":"Results"}]},
                {"type":"image"},
                {"type":"text","lines":[{"text":"people’s  bonds differ-"},{"text":"ences"}]},
                {"type":"text","lines":[{"text":"   "}]}
            ]},
            {"blocks":[]}
        ]}"#,
    )
    .expect("fixture parses");

    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0], vec!["Results", "people's bonds differences"]);
    assert!(pages[1].is_empty());
}
