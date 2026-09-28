use custody::{
    markdown::{UnitKind, parse_units},
    sections::is_weak_section,
};

fn weak(headings: &[&str]) -> bool {
    let section: Vec<String> = headings.iter().map(|h| (*h).to_string()).collect();
    is_weak_section(
        &section,
        &["limitations".to_owned(), "future work".to_owned()],
    )
}

#[test]
fn weak_heading_matches_regardless_of_case_and_surrounding_text() {
    assert!(weak(&["Limitations"]));
    assert!(weak(&["5. Limitations"]));
    assert!(weak(&["Limitations and Future Work"]));
    assert!(weak(&["LIMITATIONS"]));
    assert!(weak(&["future work"]));
}

#[test]
fn any_element_of_the_path_can_be_weak() {
    assert!(weak(&["Discussion", "Limitations"]));
    assert!(!weak(&["Discussion", "Onset"]));
}

#[test]
fn an_empty_weak_list_never_matches() {
    assert!(!is_weak_section(&["Limitations".to_owned()], &[]));
}

#[test]
fn paragraph_under_nested_headings_gets_full_path() {
    let md = "# Results\n\n## Onset\n\nThe finding.\n";
    let units = parse_units(md);
    let para = units
        .iter()
        .find(|unit| unit.kind == UnitKind::Paragraph)
        .unwrap();
    assert_eq!(para.section, ["Results", "Onset"]);
}

#[test]
fn heading_own_path_excludes_itself() {
    let md = "# Results\n\n## Onset\n\nText.\n";
    let units = parse_units(md);
    let onset = units.iter().find(|unit| unit.text == "Onset").unwrap();
    assert_eq!(onset.section, ["Results"]);
}

#[test]
fn top_level_heading_has_empty_path() {
    let units = parse_units("# Results\n\nText.\n");
    let results = units.iter().find(|unit| unit.text == "Results").unwrap();
    assert!(results.section.is_empty());
}

#[test]
fn sibling_headings_reset_path() {
    let md = "# A\n\n## B\n\nunder B\n\n## C\n\nunder C\n";
    let units = parse_units(md);
    let under_b = units.iter().find(|unit| unit.text == "under B").unwrap();
    let under_c = units.iter().find(|unit| unit.text == "under C").unwrap();
    assert_eq!(under_b.section, ["A", "B"]);
    assert_eq!(under_c.section, ["A", "C"]);
}

#[test]
fn no_headings_means_empty_section() {
    let units = parse_units("Just a paragraph.\n");
    assert!(units[0].section.is_empty());
}

#[test]
fn heading_text_is_normalized() {
    // Setext so the heading spans a soft break, which normalizes to one space.
    let md = "Results  and\n  Discussion\n===\n\nText.\n";
    let units = parse_units(md);
    let heading = units
        .iter()
        .find(|unit| unit.kind == UnitKind::Heading)
        .unwrap();
    assert_eq!(heading.text, "Results and Discussion");
    let para = units
        .iter()
        .find(|unit| unit.kind == UnitKind::Paragraph)
        .unwrap();
    assert_eq!(para.section, ["Results and Discussion"]);
}

#[test]
fn deeper_heading_pops_correctly() {
    let md = "# A\n\n### Deep\n\n## B\n\nunder B\n";
    let units = parse_units(md);
    let under_b = units.iter().find(|unit| unit.text == "under B").unwrap();
    assert_eq!(under_b.section, ["A", "B"]);
}

#[test]
fn skipped_heading_levels_nest_by_depth() {
    let md = "# A\n\n### Deep\n\nunder Deep\n";
    let units = parse_units(md);
    let under_deep = units.iter().find(|unit| unit.text == "under Deep").unwrap();
    assert_eq!(under_deep.section, ["A", "Deep"]);
}

#[test]
fn list_items_and_blockquotes_carry_the_path() {
    let md = "# A\n\n## B\n\n- an item\n\n> a quote\n";
    let units = parse_units(md);
    let item = units
        .iter()
        .find(|unit| unit.kind == UnitKind::ListItem)
        .unwrap();
    let quote = units
        .iter()
        .find(|unit| unit.kind == UnitKind::Blockquote)
        .unwrap();
    assert_eq!(item.section, ["A", "B"]);
    assert_eq!(quote.section, ["A", "B"]);
}
