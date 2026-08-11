use receipts::markdown::{UnitIndex, UnitKind, exact_count, parse_units, resolve_unit};

#[test]
fn keeps_gfm_table_cells_separate() {
    let units = parse_units("| A | B |\n|---|---|\n| 4.44 | Current |\n");

    assert!(
        units
            .iter()
            .any(|unit| unit.kind == UnitKind::TableCell && unit.text == "4.44")
    );
    assert!(
        units
            .iter()
            .any(|unit| unit.kind == UnitKind::TableCell && unit.text == "Current")
    );
    assert!(!units.iter().any(|unit| unit.text.contains("4.44 Current")));
}

#[test]
fn columns_count_unicode_characters_not_utf8_bytes() {
    let units = parse_units("| café | target |\n|---|---|\n");
    let target = units.iter().find(|unit| unit.text == "target").unwrap();

    assert_eq!(target.line, 1);
    assert_eq!(target.column, 9);
}

#[test]
fn preserves_inline_text_and_collapses_breaks() {
    let units = parse_units("A **strong** [linked](https://example.test)\nline.");

    assert_eq!(units.len(), 1);
    assert_eq!(units[0].kind, UnitKind::Paragraph);
    assert_eq!(units[0].text, "A strong linked line.");
    assert_eq!((units[0].line.get(), units[0].column.get()), (1, 1));
}

#[test]
fn classifies_paragraphs_inside_lists_and_blockquotes() {
    let units = parse_units("- list text\n\n> quoted text\n");

    assert!(
        units
            .iter()
            .any(|unit| unit.kind == UnitKind::ListItem && unit.text == "list text")
    );
    assert!(
        units
            .iter()
            .any(|unit| unit.kind == UnitKind::Blockquote && unit.text == "quoted text")
    );
}

#[test]
fn treats_br_as_whitespace_and_ignores_formatting_tags() {
    let units = parse_units("before<br><span>inside</span> after");

    assert_eq!(units[0].text, "before inside after");
}

#[test]
fn ignores_multibyte_inline_html_without_panicking() {
    let units = parse_units("para <?é?> tail");

    assert_eq!(units[0].text, "para tail");
}

#[test]
fn emits_heading_and_code_block_units() {
    let units = parse_units("# Heading\n\n```text\nalpha  beta\n```\n");

    assert_eq!(units[0].kind, UnitKind::Heading);
    assert_eq!(units[0].text, "Heading");
    assert_eq!(units[1].kind, UnitKind::CodeBlock);
    assert_eq!(units[1].text, "alpha beta");
}

#[test]
fn resolves_units_by_exact_coordinates_and_kind() {
    let units = parse_units("first\n\nsecond\n");
    let index = UnitIndex::new(&units);
    let second = &units[1];

    assert_eq!((second.line.get(), second.column.get()), (3, 1));
    assert_eq!(
        resolve_unit(
            &units,
            &index,
            UnitKind::Paragraph,
            second.line,
            second.column,
        )
        .unwrap()
        .text,
        "second"
    );
    assert!(
        resolve_unit(
            &units,
            &index,
            UnitKind::Heading,
            second.line,
            second.column,
        )
        .is_err()
    );

    let mut ambiguous = units.clone();
    ambiguous.push(second.clone());
    let index = UnitIndex::new(&ambiguous);
    let error = resolve_unit(
        &ambiguous,
        &index,
        UnitKind::Paragraph,
        second.line,
        second.column,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("multiple paragraph units"), "{error}");
}

#[test]
fn counts_zero_one_and_multiple_exact_occurrences() {
    assert_eq!(exact_count("alpha beta", "gamma"), 0);
    assert_eq!(exact_count("alpha beta", "alpha"), 1);
    assert_eq!(exact_count("same same", "same"), 2);
    assert_eq!(exact_count("aaaa", "aa"), 2);
}
