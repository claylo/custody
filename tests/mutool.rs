use custody::pdf::{
    matching_bbox,
    mutool::{parse_stext, parse_stext_blocks, validate_version},
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
    let pages = parse_stext(include_str!("fixtures/mutool-prose.xml")).expect("fixture parses");

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
    let pages =
        parse_stext(include_str!("fixtures/mutool-rotated-table.xml")).expect("fixture parses");

    assert!(!pages[0].text.contains("42.8% within tolerance"));
    assert!(pages[0].text.contains("42 . 8 % within tolera nce"));
}

/// `MuPDF`'s JSON writer splits a typeset line at every font change, which
/// turned publisher font-subset seams (`attachme` + `nt`, `Bowlby` + `’` + `s`)
/// into spaces. The XML `<line text="…">` attribute is the whole line.
#[test]
fn keeps_font_runs_inside_one_line() {
    let pages = parse_stext(include_str!("fixtures/mutool-font-runs.xml")).expect("fixture parses");

    let text = &pages[0].text;
    assert!(text.contains("informed by attachment theory and research"));
    assert!(text.contains("congruently with Bowlby's attachment"));
    assert!(!text.contains("attachme nt"));
    assert!(!text.contains("Bowlby ' s"));
    // Attribute entities are decoded and the hyphenated block still rejoins.
    assert!(text.contains("sequelae, considered from the perspective of Ainsworth & Wall (1978)."));
    // Bounding boxes come from the x0 y0 x1 y1 attribute form.
    let bbox = matching_bbox(&pages[0], "congruently with Bowlby's attachment").unwrap();
    assert_eq!(
        (bbox.x, bbox.y, bbox.width, bbox.height),
        (59.9811, 230.8231, 347.8461 - 59.9811, 239.2021 - 230.8231)
    );
}

#[test]
fn rejects_output_without_pages() {
    let error = parse_stext(r#"<?xml version="1.0"?><document filename="empty.pdf"></document>"#)
        .expect_err("pages are required");

    assert!(error.to_string().contains("pages"));
}

#[test]
fn rejects_malformed_xml() {
    let error = parse_stext(r#"<document><page id="page1"><block><line text="x"></page>"#)
        .expect_err("mismatched tags are rejected");

    assert!(error.to_string().contains("structured-text"));
}

#[test]
fn rejects_lines_without_text() {
    let error = parse_stext(
        r#"<document><page id="page1"><block><line bbox="1 2 3 4"></line></block></page></document>"#,
    )
    .expect_err("the text attribute is required");

    assert!(error.to_string().contains("text"));
}

#[test]
fn preserves_non_text_blocks_for_ocr_fallback() {
    let pages = parse_stext(
        r#"<document><page id="page1" width="504" height="720"><image bbox="0 0 504 720"/></page></document>"#,
    )
    .expect("image-only pages remain valid native extraction results");

    assert!(pages[0].text.is_empty());
    assert!(pages[0].spans.is_empty());
}

#[test]
fn preserves_lines_without_geometry() {
    let pages = parse_stext(
        r#"<document><page id="page1"><block><line text="plain"></line></block></page></document>"#,
    )
    .expect("bbox remains optional");

    assert_eq!(pages[0].text, "plain");
    assert_eq!(pages[0].spans[0].bbox, None);
}

#[test]
fn rejoins_words_hyphenated_across_stext_lines() {
    let pages = parse_stext(
        r#"<document><page id="page1"><block>
            <line bbox="10 20 210 32" text="The infant showed an attach-"></line>
            <line bbox="10 34 230 46" text="ment behaviour toward the mother."></line>
        </block></page></document>"#,
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
    let pages = parse_stext_blocks(
        r#"<document>
            <page id="page1">
                <block><line text="Results"></line></block>
                <image bbox="0 0 10 10"/>
                <block><line text="people’s  bonds differ-"></line><line text="ences"></line></block>
                <block><line text="   "></line></block>
            </page>
            <page id="page2"></page>
        </document>"#,
    )
    .expect("fixture parses");

    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0], vec!["Results", "people's bonds differences"]);
    assert!(pages[1].is_empty());
}
