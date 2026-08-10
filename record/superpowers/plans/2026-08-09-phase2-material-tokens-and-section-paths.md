# Phase 2: Material Tokens and Section Paths — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add required-token coverage checking and section path recording/verification so that claim locators must account for what the claim asserts, and locators record which Markdown heading tree they sit under.

**Architecture:** Three new modules: `src/tokens.rs` (extract required/advisory tokens from claim text, check coverage against locator `exact` values), `src/sections.rs` (heading-stack tracking during Markdown parsing, path verification). Config gains `coverage` and `sections` blocks; `EvidenceIssue` gains a `severity` field to distinguish warnings from errors. Validation calls the token and section logic per-claim, per-locator.

**Tech Stack:** Rust, pulldown-cmark (already a dep), serde, existing test harness patterns.

**Spec reference:** `record/superpowers/specs/2026-07-30-multi-source-and-support-design.md`, Phase 2 section (lines 234–349).

---

## File Map

| Action | File | Responsibility |
|--------|------|----------------|
| Create | `src/tokens.rs` | Token extraction from claim text, coverage checking against locator exact values |
| Create | `src/sections.rs` | Heading-stack tracking during markdown parse, path comparison |
| Modify | `src/lib.rs` | Register new modules |
| Modify | `src/config.rs` | Add `coverage` and `sections` config blocks |
| Modify | `src/evidence.rs` | Add `severity` to `EvidenceIssue`; add `section` to `MarkdownLocator` |
| Modify | `src/markdown.rs` | Add `section` field to `MarkdownUnit`; pass heading level through to stack tracking |
| Modify | `src/validate.rs` | Wire token coverage and section verification into `validate_document` |
| Modify | `src/cli.rs` | Pass config to `validate_document`; distinguish warning vs error in exit status |
| Create | `tests/tokens.rs` | Token extraction and coverage unit tests |
| Create | `tests/sections.rs` | Section path tracking and verification tests |
| Modify | `tests/validate.rs` | Integration tests for new error codes |
| Modify | `tests/evidence.rs` | Update `MarkdownLocator` construction to include `section` |

---

### Task 1: Add `severity` to `EvidenceIssue`

The spec defines `weak_section_only` as permanently a warning and `uncovered_token` as configurable. Today `EvidenceIssue` has no severity concept. Every issue is implicitly an error. Adding severity here first means all downstream code can use it immediately.

**Files:**
- Modify: `src/evidence.rs:97-105` (the `EvidenceIssue` struct)
- Modify: `src/evidence.rs:380-392` (the `issue()` helper)
- Modify: `src/validate.rs:346-358` (the `issue()` helper there)
- Modify: `src/cli.rs` (exit-status logic, issue printing)
- Modify: `tests/evidence.rs`
- Modify: `tests/validate.rs`

- [ ] **Step 1: Add `Severity` enum and update `EvidenceIssue`**

In `src/evidence.rs`, add a `Severity` enum and a `severity` field to `EvidenceIssue`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}
```

Add `pub severity: Severity` to `EvidenceIssue`, between `code` and `message`. Update the `issue()` helper in `evidence.rs` to accept a severity parameter:

```rust
fn issue(
    code: impl Into<String>,
    severity: Severity,
    message: impl Into<String>,
    claim: Option<usize>,
    locator: Option<usize>,
) -> EvidenceIssue {
    EvidenceIssue {
        code: code.into(),
        severity,
        message: message.into(),
        claim,
        locator,
    }
}
```

Update every call site in `evidence.rs` (`validate_evidence_structure`, `validate_source`, `validate_hash`, `validate_locator`) to pass `Severity::Error` — all existing issues are errors.

- [ ] **Step 2: Update the `issue()` helper in `validate.rs`**

Same change: add `severity` parameter, pass `Severity::Error` at all existing call sites. Import `Severity` from `crate::evidence`.

```rust
fn issue(
    code: impl Into<String>,
    severity: Severity,
    message: impl Into<String>,
    claim: Option<usize>,
    locator: Option<usize>,
) -> EvidenceIssue {
    EvidenceIssue {
        code: code.into(),
        severity,
        message: message.into(),
        claim,
        locator,
    }
}
```

- [ ] **Step 3: Update `ValidationReport::is_valid` to ignore warnings**

In `src/validate.rs`, change `is_valid` from checking `issues.is_empty()` to checking that no issue has `Severity::Error`:

```rust
#[must_use]
pub fn is_valid(&self) -> bool {
    !self.issues.iter().any(|i| i.severity == Severity::Error)
}

#[must_use]
pub fn has_warnings(&self) -> bool {
    self.issues.iter().any(|i| i.severity == Severity::Warning)
}
```

- [ ] **Step 4: Update `cli.rs` issue printing to label warnings**

In `print_issues` and `print_audit_issues`, prefix warnings with `warning:`:

```rust
fn print_issues(reports: &[ValidationReport]) {
    for report in reports {
        for issue in &report.issues {
            let prefix = match issue.severity {
                Severity::Warning => "warning: ",
                Severity::Error => "",
            };
            eprintln!("{}: {}{}: {}", report.id, prefix, issue.code, issue.message);
        }
    }
}
```

Same pattern for `print_audit_issues`. Also update the `AuditSummary` construction in `audit()` and the `error_report()` helper in `cli.rs` to include severity.

- [ ] **Step 5: Fix test compilation**

Update `tests/evidence.rs` and `tests/validate.rs` — anywhere `EvidenceIssue` is constructed directly (the `AuditSummary` in `cli.rs` has two inline constructions that also need updating). The easiest approach: add `severity: Severity::Error` to every existing `EvidenceIssue` literal. Import `Severity` from `receipts::evidence`.

- [ ] **Step 6: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

Expected: all 76 tests pass.

- [ ] **Step 7: Commit**

```
feat(evidence): add severity field to EvidenceIssue

All existing issues are errors. Warnings will be used by
uncovered_token (configurable) and weak_section_only (Phase 2).
```

---

### Task 2: Add `section` field to `MarkdownUnit` and `MarkdownLocator`

The spec says `MarkdownUnit` gains `section: Vec<String>` — ancestor headings, and `MarkdownLocator` gains a `section` field for recording and re-verifying.

**Files:**
- Modify: `src/markdown.rs:21-26` (`MarkdownUnit` struct)
- Modify: `src/markdown.rs:36-138` (`parse_units` function)
- Modify: `src/evidence.rs:64-70` (`MarkdownLocator` struct)
- Modify: `tests/markdown.rs`
- Modify: `tests/evidence.rs` (update `MarkdownLocator` construction)
- Modify: `tests/validate.rs` (update `MarkdownLocator` construction)

- [ ] **Step 1: Add `section` to `MarkdownUnit`**

In `src/markdown.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownUnit {
    pub kind: UnitKind,
    pub text: String,
    pub line: usize,
    pub column: usize,
    pub section: Vec<String>,
}
```

- [ ] **Step 2: Track heading stack in `parse_units`**

Add a heading stack to `parse_units`. The stack is `Vec<(u8, String)>` where the `u8` is the heading level (1–6). When a `Heading` unit finishes:

1. Pop all entries from the stack with level >= this heading's level.
2. The unit's `section` is the current stack's text values (before pushing).
3. Push `(level, normalized_text)` onto the stack.

Non-heading units get `section` cloned from the current stack state.

In the `parse_units` function, add:

```rust
let mut heading_stack: Vec<(u8, String)> = Vec::new();
```

Track the current heading level inside the `UnitBuilder`:

```rust
#[derive(Debug)]
struct UnitBuilder {
    kind: UnitKind,
    text: String,
    start: usize,
    heading_level: Option<u8>,
}
```

When `Tag::Heading { level, .. }` starts, store the level:

```rust
Tag::Heading { level, .. } if html_block_depth == 0 => {
    let mut builder = UnitBuilder::new(UnitKind::Heading, range.start);
    builder.heading_level = Some(level as u8);
    active = Some(builder);
}
```

Update `finish_unit` to accept and mutate the heading stack, and return the section path:

```rust
fn finish_unit(
    source: &str,
    active: &mut Option<UnitBuilder>,
    units: &mut Vec<MarkdownUnit>,
    heading_stack: &mut Vec<(u8, String)>,
) {
    let Some(builder) = active.take() else {
        return;
    };
    let text = normalize(&builder.text);
    if text.is_empty() {
        return;
    }
    let (line, column) = line_column(source, builder.start);

    let section = if let Some(level) = builder.heading_level {
        let section: Vec<String> = heading_stack.iter().map(|(_, t)| t.clone()).collect();
        heading_stack.retain(|(l, _)| *l < level);
        heading_stack.push((level, text.clone()));
        section
    } else {
        heading_stack.iter().map(|(_, t)| t.clone()).collect()
    };

    units.push(MarkdownUnit {
        kind: builder.kind,
        text,
        line,
        column,
        section,
    });
}
```

Update all `finish_unit` call sites to pass `&mut heading_stack`.

- [ ] **Step 3: Add `section` to `MarkdownLocator`**

In `src/evidence.rs`, add an optional section field:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkdownLocator {
    pub line: usize,
    pub column: usize,
    pub unit: UnitKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub section: Vec<String>,
}
```

`default` + `skip_serializing_if` means existing YAML without `section` still parses, and output only includes it when non-empty. This is backward compatible with all existing evidence records.

- [ ] **Step 4: Write section path tests**

Create `tests/sections.rs`:

```rust
use receipts::markdown::{UnitKind, parse_units};

#[test]
fn paragraph_under_nested_headings_gets_full_path() {
    let md = "# Results\n\n## Onset\n\nThe finding.\n";
    let units = parse_units(md);
    let para = units.iter().find(|u| u.kind == UnitKind::Paragraph).unwrap();
    assert_eq!(para.section, ["Results", "Onset"]);
}

#[test]
fn heading_own_path_excludes_itself() {
    let md = "# Results\n\n## Onset\n\nText.\n";
    let units = parse_units(md);
    let onset = units.iter().find(|u| u.text == "Onset").unwrap();
    assert_eq!(onset.section, ["Results"]);
}

#[test]
fn sibling_headings_reset_path() {
    let md = "# A\n\n## B\n\nunder B\n\n## C\n\nunder C\n";
    let units = parse_units(md);
    let under_b = units.iter().find(|u| u.text == "under B").unwrap();
    let under_c = units.iter().find(|u| u.text == "under C").unwrap();
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
    let md = "# Results  and\n  Discussion\n\nText.\n";
    let units = parse_units(md);
    let h = units.iter().find(|u| u.kind == UnitKind::Heading).unwrap();
    assert_eq!(h.text, "Results and Discussion");
    let para = units.iter().find(|u| u.kind == UnitKind::Paragraph).unwrap();
    assert_eq!(para.section, ["Results and Discussion"]);
}

#[test]
fn deeper_heading_pops_correctly() {
    let md = "# A\n\n### Deep\n\n## B\n\nunder B\n";
    let units = parse_units(md);
    let under_b = units.iter().find(|u| u.text == "under B").unwrap();
    assert_eq!(under_b.section, ["A", "B"]);
}
```

- [ ] **Step 5: Fix existing `MarkdownLocator` construction in tests**

In `tests/evidence.rs` and `tests/validate.rs`, add `section: vec![]` to every `MarkdownLocator` literal. In the `locator()` helper in `tests/validate.rs`:

```rust
fn locator(source: &str, exact: &str) -> Locator {
    Locator {
        source: source.to_owned(),
        exact: exact.to_owned(),
        markdown: MarkdownLocator {
            line: 1,
            column: 1,
            unit: UnitKind::Paragraph,
            section: vec![],
        },
        pdf: PdfLocator {
            page: 1,
            backend: PdfBackend::MutoolNative,
        },
    }
}
```

Same for the `summary()` method in `Fixture` and any other `MarkdownLocator` literals.

- [ ] **Step 6: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

Expected: all tests pass (existing + 6 new section tests).

- [ ] **Step 7: Commit**

```
feat(markdown): track section paths through heading stack

MarkdownUnit now carries a section path (ancestor heading texts).
MarkdownLocator gains an optional section field for recording
and re-verifying. Backward compatible: missing section defaults
to empty.
```

---

### Task 3: Section path verification in `validate.rs`

After the Markdown unit is resolved, compare its computed `section` to the recorded `section` in the locator. A mismatch is `stale_section`.

**Files:**
- Create: `src/sections.rs`
- Modify: `src/lib.rs`
- Modify: `src/validate.rs:128-160`
- Modify: `tests/validate.rs`

- [ ] **Step 1: Create `src/sections.rs` with path comparison**

```rust
pub fn paths_match(recorded: &[String], computed: &[String]) -> bool {
    recorded == computed
}
```

Simple equality. The spec says "a recomputed path that differs from the record is `stale_section`." No fuzzy matching.

- [ ] **Step 2: Register the module in `src/lib.rs`**

Add `pub mod sections;` to `src/lib.rs`.

- [ ] **Step 3: Add section verification to `validate_document`**

In `src/validate.rs`, after the `resolve_unit` Ok arm where `exact_count` is checked, add section verification when the locator has a non-empty recorded section:

```rust
Ok(unit) => {
    match exact_count(&unit.text, &locator.exact) {
        1 => {
            if !locator.markdown.section.is_empty()
                && !sections::paths_match(&locator.markdown.section, &unit.section)
            {
                issues.push(issue(
                    "stale_section",
                    Severity::Error,
                    format!(
                        "recorded section {:?} but unit is under {:?}",
                        locator.markdown.section, unit.section
                    ),
                    Some(entry.claim),
                    Some(locator_index),
                ));
            }
        }
        0 => { /* existing markdown_missing */ }
        count => { /* existing markdown_ambiguous */ }
    }
}
```

Import `sections` at the top of `validate.rs`.

- [ ] **Step 4: Write stale_section test**

In `tests/validate.rs`:

```rust
#[test]
fn stale_section_is_reported_when_path_changes() {
    let fixture = Fixture::new("# Results\n\n## Onset\n\nSupported once in the text.\n");
    let mut summary = fixture.summary("Supported once", PdfBackend::MutoolNative);
    let locator = &mut summary.evidence.as_mut().unwrap().claims[0].locators[0];
    locator.markdown.line = 5;
    locator.markdown.section = vec!["Results".to_owned(), "Discussion".to_owned()];

    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([((PdfBackend::MutoolNative, 1), "Supported once.".to_owned())]),
        },
    );

    assert!(
        report.issues.iter().any(|i| i.code == "stale_section"),
        "{:?}",
        report.issues
    );
}

#[test]
fn matching_section_produces_no_issue() {
    let fixture = Fixture::new("# Results\n\n## Onset\n\nSupported once in the text.\n");
    let mut summary = fixture.summary("Supported once", PdfBackend::MutoolNative);
    let locator = &mut summary.evidence.as_mut().unwrap().claims[0].locators[0];
    locator.markdown.line = 5;
    locator.markdown.section = vec!["Results".to_owned(), "Onset".to_owned()];

    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([((PdfBackend::MutoolNative, 1), "Supported once.".to_owned())]),
        },
    );

    assert!(
        !report.issues.iter().any(|i| i.code == "stale_section"),
        "{:?}",
        report.issues
    );
}
```

- [ ] **Step 5: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

- [ ] **Step 6: Commit**

```
feat(validate): verify section paths with stale_section error

A recorded section path that disagrees with the computed path
from the Markdown source is now reported as stale_section.
```

---

### Task 4: Token extraction — `src/tokens.rs`

Extract required and advisory tokens from claim text. This is the core of the coverage check.

**Files:**
- Create: `src/tokens.rs`
- Modify: `src/lib.rs`
- Create: `tests/tokens.rs`

- [ ] **Step 1: Create `src/tokens.rs` with extraction functions**

```rust
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedTokens {
    pub required: Vec<String>,
    pub advisory: Vec<String>,
}

pub fn extract(claim: &str) -> ExtractedTokens {
    let mut required = BTreeSet::new();
    required.extend(numeric_literals(claim));
    required.extend(number_words(claim));
    required.extend(quoted_phrases(claim));
    let advisory = capitalized_multiword_terms(claim);
    ExtractedTokens {
        required: required.into_iter().collect(),
        advisory,
    }
}

fn numeric_literals(text: &str) -> Vec<String> {
    let mut results = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b',')
            {
                i += 1;
            }
            // Include trailing %
            if i < bytes.len() && bytes[i] == b'%' {
                i += 1;
            }
            let mut token = &text[start..i];
            // Trim trailing . and ,
            token = token.trim_end_matches(|c| c == '.' || c == ',');
            if !token.is_empty() {
                results.push(token.to_owned());
            }
        } else {
            i += 1;
        }
    }
    results
}

const NUMBER_WORDS: &[&str] = &[
    "zero", "one", "two", "three", "four", "five", "six", "seven",
    "eight", "nine", "ten", "eleven", "twelve", "thirteen", "fourteen",
    "fifteen", "sixteen", "seventeen", "eighteen", "nineteen", "twenty",
    "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    "hundred", "thousand", "million", "billion",
];

fn number_words(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut found = Vec::new();
    for word in NUMBER_WORDS {
        if word_boundary_match(&lower, word) {
            found.push(word.to_string());
        }
    }
    found
}

fn quoted_phrases(text: &str) -> Vec<String> {
    let mut results = Vec::new();
    for delimiter in ['"', '\''] {
        let mut chars = text.char_indices().peekable();
        while let Some((start, ch)) = chars.next() {
            if ch == delimiter {
                let content_start = start + ch.len_utf8();
                let mut end = None;
                for (idx, inner) in chars.by_ref() {
                    if inner == delimiter {
                        end = Some(idx);
                        break;
                    }
                }
                if let Some(end_idx) = end {
                    let phrase = &text[content_start..end_idx];
                    let normalized = crate::normalize::normalize(phrase);
                    if !normalized.is_empty() {
                        results.push(normalized);
                    }
                }
            }
        }
    }
    results
}

fn capitalized_multiword_terms(text: &str) -> Vec<String> {
    let mut results = Vec::new();
    let sentences: Vec<&str> = text.split(|c: char| c == '.' || c == '!' || c == '?')
        .collect();
    for sentence in &sentences {
        let trimmed = sentence.trim();
        if trimmed.is_empty() {
            continue;
        }
        let words: Vec<&str> = trimmed.split_whitespace().collect();
        let mut run_start = None;
        let mut run_len = 0;
        for (i, word) in words.iter().enumerate() {
            let is_capitalized = word.starts_with(|c: char| c.is_uppercase())
                && word.chars().skip(1).any(|c| c.is_lowercase());
            let is_sentence_start = i == 0;
            if is_capitalized && !is_sentence_start {
                if run_start.is_none() {
                    run_start = Some(i);
                    run_len = 1;
                } else {
                    run_len += 1;
                }
            } else if is_capitalized && is_sentence_start {
                // Don't start a run at sentence start, but allow continuing
                // from the next word
            } else {
                if run_len >= 2 {
                    let start = run_start.unwrap();
                    let term: String = words[start..start + run_len].join(" ");
                    results.push(term);
                }
                run_start = None;
                run_len = 0;
            }
        }
        if run_len >= 2 {
            let start = run_start.unwrap();
            let term: String = words[start..start + run_len].join(" ");
            results.push(term);
        }
    }
    results
}

/// Check whether `token` is covered in `text` with word boundaries.
///
/// A token is covered when it appears delimited on both sides by a
/// non-ASCII-alphanumeric character or a string edge.
pub fn is_covered(text: &str, token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    for (idx, _) in text.match_indices(token) {
        let before_ok = idx == 0
            || !text.as_bytes()[idx - 1].is_ascii_alphanumeric();
        let after = idx + token.len();
        let after_ok = after >= text.len()
            || !text.as_bytes()[after].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

/// Case-insensitive version of `is_covered`, for number words.
pub fn is_covered_case_insensitive(text: &str, token: &str) -> bool {
    let lower_text = text.to_lowercase();
    let lower_token = token.to_lowercase();
    is_covered(&lower_text, &lower_token)
}

fn word_boundary_match(text: &str, word: &str) -> bool {
    is_covered(text, word)
}
```

- [ ] **Step 2: Register module in `lib.rs`**

Add `pub mod tokens;` to `src/lib.rs`.

- [ ] **Step 3: Write extraction tests**

Create `tests/tokens.rs`:

```rust
use receipts::tokens::{extract, is_covered, is_covered_case_insensitive};

#[test]
fn extracts_decimal_with_percent() {
    let tokens = extract("The rate was 67.5% in the control group.");
    assert!(tokens.required.contains(&"67.5%".to_owned()));
}

#[test]
fn extracts_thousands_separator() {
    let tokens = extract("There were 1,228 participants.");
    assert!(tokens.required.contains(&"1,228".to_owned()));
}

#[test]
fn extracts_plain_integer() {
    let tokens = extract("Over 3 trials the result held.");
    assert!(tokens.required.contains(&"3".to_owned()));
}

#[test]
fn extracts_decimal_without_percent() {
    let tokens = extract("p-value was 0.05.");
    assert!(tokens.required.contains(&"0.05".to_owned()));
}

#[test]
fn trims_trailing_punctuation_from_numbers() {
    let tokens = extract("There were 42.");
    assert!(tokens.required.contains(&"42".to_owned()));
    assert!(!tokens.required.iter().any(|t| t.contains('.')));
}

#[test]
fn extracts_number_words_case_insensitively() {
    let tokens = extract("Three cohorts were tested against Two baselines.");
    assert!(tokens.required.contains(&"three".to_owned()));
    assert!(tokens.required.contains(&"two".to_owned()));
}

#[test]
fn extracts_scale_words() {
    let tokens = extract("Over one hundred thousand samples.");
    assert!(tokens.required.contains(&"one".to_owned()));
    assert!(tokens.required.contains(&"hundred".to_owned()));
    assert!(tokens.required.contains(&"thousand".to_owned()));
}

#[test]
fn extracts_quoted_phrases() {
    let tokens = extract("The study found \"no significant effect\" in the data.");
    assert!(tokens.required.contains(&"no significant effect".to_owned()));
}

#[test]
fn extracts_single_quoted_phrases() {
    let tokens = extract("The method is called 'gradient descent' here.");
    assert!(tokens.required.contains(&"gradient descent".to_owned()));
}

#[test]
fn quoted_phrase_is_normalized() {
    let tokens = extract("Found \"excess   whitespace\" in output.");
    assert!(tokens.required.contains(&"excess whitespace".to_owned()));
}

#[test]
fn advisory_captures_capitalized_multiword_not_at_sentence_start() {
    let tokens = extract("The Global Carbon Budget increased sharply.");
    assert!(tokens.advisory.contains(&"Global Carbon".to_owned())
        || tokens.advisory.contains(&"Global Carbon Budget".to_owned()));
}

#[test]
fn sentence_start_capitalization_is_not_advisory() {
    let tokens = extract("Results were clear.");
    assert!(tokens.advisory.is_empty());
}

#[test]
fn boundary_check_prevents_substring_match() {
    assert!(!is_covered("13 regimes", "3"));
    assert!(is_covered("3 regimes", "3"));
    assert!(is_covered("value=3", "3"));
    assert!(!is_covered("x13y", "3"));
}

#[test]
fn boundary_at_string_edges() {
    assert!(is_covered("3", "3"));
    assert!(is_covered("hello 3", "3"));
    assert!(is_covered("3 hello", "3"));
}

#[test]
fn case_insensitive_coverage_for_number_words() {
    assert!(is_covered_case_insensitive("Three regimes were tested", "three"));
    assert!(is_covered_case_insensitive("THREE REGIMES", "three"));
    assert!(!is_covered_case_insensitive("thirteenth trial", "three"));
}

#[test]
fn empty_claim_yields_no_tokens() {
    let tokens = extract("");
    assert!(tokens.required.is_empty());
    assert!(tokens.advisory.is_empty());
}
```

- [ ] **Step 4: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

Expected: all existing tests plus ~17 new token tests pass.

- [ ] **Step 5: Commit**

```
feat(tokens): extract required and advisory tokens from claim text

Numeric literals, number words, quoted phrases are required.
Capitalized multiword terms are advisory. Word-boundary matching
prevents substring false positives.
```

---

### Task 5: Add `coverage` and `sections` config

The spec defines `coverage.tokens` (error/warn/off, default error) and `sections.weak` (list of strings).

**Files:**
- Modify: `src/config.rs`
- Modify: `tests/foundation.rs` (if config tests live there)

- [ ] **Step 1: Add config structs**

In `src/config.rs`, add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenSeverity {
    Error,
    Warn,
    Off,
}

impl Default for TokenSeverity {
    fn default() -> Self {
        Self::Error
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CoverageConfig {
    pub tokens: TokenSeverity,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SectionsConfig {
    #[serde(default)]
    pub weak: Vec<String>,
}
```

Add both to `Config`:

```rust
pub struct Config {
    pub corpus: CorpusLayout,
    pub cache: CacheConfig,
    pub pdf: PdfConfig,
    pub terms: crate::terms::Terms,
    pub coverage: CoverageConfig,
    pub sections: SectionsConfig,
}
```

- [ ] **Step 2: Thread config through `Corpus`**

Add `coverage_config` and `sections_config` fields to `Corpus`, with accessor methods, following the same pattern as `ocr_config()`:

```rust
pub struct Corpus {
    root: PathBuf,
    layout: CorpusLayout,
    cache_root: PathBuf,
    config_file: Option<PathBuf>,
    terms: crate::terms::Terms,
    ocr_config: crate::config::OcrConfig,
    coverage_config: crate::config::CoverageConfig,
    sections_config: crate::config::SectionsConfig,
}
```

In `from_discovered`, assign them from `config.coverage` and `config.sections`.

Add accessor methods:

```rust
#[must_use]
pub const fn coverage_config(&self) -> &crate::config::CoverageConfig {
    &self.coverage_config
}

#[must_use]
pub fn sections_config(&self) -> &crate::config::SectionsConfig {
    &self.sections_config
}
```

- [ ] **Step 3: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

- [ ] **Step 4: Commit**

```
feat(config): add coverage and sections configuration blocks

coverage.tokens: error|warn|off (default error)
sections.weak: list of heading substring matches (default empty)
```

---

### Task 6: Wire token coverage into validation

This is the integration point: `validate_document` checks that each claim's required tokens are covered by its locators, using the configured severity.

**Files:**
- Modify: `src/validate.rs`
- Modify: `tests/validate.rs`

- [ ] **Step 1: Add token coverage check to `validate_document`**

In `src/validate.rs`, after the per-locator loop for each claim entry, add token coverage checking. The check runs per-claim, not per-locator — a required token is covered if *any* locator for that claim contains it.

After the existing `for entry in &evidence.claims { ... }` loop's locator iteration, add:

```rust
use crate::config::TokenSeverity;
use crate::tokens;

// Inside validate_document, after source resolution but interleaved with claim processing:
let token_severity = corpus.coverage_config().tokens;

// Inside the `for entry in &evidence.claims` loop, after the locator loop:
if token_severity != TokenSeverity::Off {
    if let Some(claim_text) = summary.claims.get(entry.claim) {
        let extracted = tokens::extract(claim_text);
        let severity = match token_severity {
            TokenSeverity::Error => Severity::Error,
            TokenSeverity::Warn => Severity::Warning,
            TokenSeverity::Off => unreachable!(),
        };
        for token in &extracted.required {
            let covered = entry.locators.iter().any(|loc| {
                if NUMBER_WORDS.contains(&token.as_str()) {
                    tokens::is_covered_case_insensitive(&loc.exact, token)
                } else {
                    tokens::is_covered(&loc.exact, token)
                }
            });
            if !covered {
                issues.push(issue(
                    "uncovered_token",
                    severity,
                    format!(
                        "required token {:?} from claim {} appears in no locator",
                        token, entry.claim
                    ),
                    Some(entry.claim),
                    None,
                ));
            }
        }
    }
}
```

Note: `NUMBER_WORDS` needs to be accessible. Export the constant from `tokens.rs` or add a helper method `tokens::is_number_word(token) -> bool` to keep the logic encapsulated. Prefer the helper:

In `src/tokens.rs`, add:

```rust
#[must_use]
pub fn is_number_word(token: &str) -> bool {
    NUMBER_WORDS.contains(&token)
}
```

Then the check becomes:

```rust
let covered = entry.locators.iter().any(|loc| {
    if tokens::is_number_word(token) {
        tokens::is_covered_case_insensitive(&loc.exact, token)
    } else {
        tokens::is_covered(&loc.exact, token)
    }
});
```

- [ ] **Step 2: Write token coverage validation tests**

In `tests/validate.rs`:

```rust
#[test]
fn uncovered_token_is_reported_when_number_missing_from_locators() {
    let fixture = Fixture::new("The effect was 42.8% within tolerance.\n");
    let claim = "The result showed 42.8% accuracy across 3 trials.".to_owned();
    let summary = SummaryDocument {
        id: "smith-2019".to_owned(),
        claims: vec![claim.clone()],
        evidence: Some(Evidence {
            sources: BTreeMap::from([(
                DEFAULT_SOURCE.to_owned(),
                SourcePair {
                    markdown: SourceRecord {
                        source: "md/smith-2019/smith-2019.md".to_owned(),
                        sha256: sha256_bytes(fixture.markdown.as_bytes()),
                    },
                    pdf: SourceRecord {
                        source: "pdfs/smith-2019.pdf".to_owned(),
                        sha256: sha256_file(&fixture.corpus.root().join("pdfs/smith-2019.pdf"))
                            .unwrap(),
                    },
                },
            )]),
            claims: vec![ClaimEvidence {
                claim: 0,
                claim_sha256: sha256_bytes(claim.as_bytes()),
                locators: vec![Locator {
                    source: DEFAULT_SOURCE.to_owned(),
                    exact: "42.8% within tolerance".to_owned(),
                    markdown: MarkdownLocator {
                        line: 1,
                        column: 1,
                        unit: UnitKind::Paragraph,
                        section: vec![],
                    },
                    pdf: PdfLocator {
                        page: 1,
                        backend: PdfBackend::MutoolNative,
                    },
                }],
            }],
        }),
    };

    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([(
                (PdfBackend::MutoolNative, 1),
                "The effect was 42.8% within tolerance.".to_owned(),
            )]),
        },
    );

    let uncovered: Vec<_> = report
        .issues
        .iter()
        .filter(|i| i.code == "uncovered_token")
        .collect();
    // "42.8%" is covered, but "3" is not in any locator
    assert!(
        uncovered.iter().any(|i| i.message.contains("\"3\"")),
        "{:?}",
        report.issues
    );
    assert!(
        !uncovered.iter().any(|i| i.message.contains("42.8%")),
        "{:?}",
        report.issues
    );
}
```

- [ ] **Step 3: Test coverage.tokens: warn and off**

This needs a `Fixture::with_config` variant or adjustments. Add tests:

```rust
#[test]
fn coverage_tokens_off_skips_token_check() {
    let fixture = Fixture::with_config(
        "The finding text.\n",
        "cache:\n  root: \".cache/pdf-text\"\ncoverage:\n  tokens: off\n",
    );
    let claim = "There were 5 total findings.".to_owned();
    let summary = SummaryDocument {
        id: "smith-2019".to_owned(),
        claims: vec![claim.clone()],
        evidence: Some(Evidence {
            sources: BTreeMap::from([(
                DEFAULT_SOURCE.to_owned(),
                SourcePair {
                    markdown: SourceRecord {
                        source: "md/smith-2019/smith-2019.md".to_owned(),
                        sha256: sha256_bytes(fixture.markdown.as_bytes()),
                    },
                    pdf: SourceRecord {
                        source: "pdfs/smith-2019.pdf".to_owned(),
                        sha256: sha256_file(&fixture.corpus.root().join("pdfs/smith-2019.pdf"))
                            .unwrap(),
                    },
                },
            )]),
            claims: vec![ClaimEvidence {
                claim: 0,
                claim_sha256: sha256_bytes(claim.as_bytes()),
                locators: vec![Locator {
                    source: DEFAULT_SOURCE.to_owned(),
                    exact: "finding text".to_owned(),
                    markdown: MarkdownLocator {
                        line: 1,
                        column: 1,
                        unit: UnitKind::Paragraph,
                        section: vec![],
                    },
                    pdf: PdfLocator {
                        page: 1,
                        backend: PdfBackend::MutoolNative,
                    },
                }],
            }],
        }),
    };

    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([(
                (PdfBackend::MutoolNative, 1),
                "The finding text.".to_owned(),
            )]),
        },
    );

    assert!(
        !report.issues.iter().any(|i| i.code == "uncovered_token"),
        "{:?}",
        report.issues
    );
}

#[test]
fn coverage_tokens_warn_produces_warnings_not_errors() {
    let fixture = Fixture::with_config(
        "The finding text.\n",
        "cache:\n  root: \".cache/pdf-text\"\ncoverage:\n  tokens: warn\n",
    );
    let claim = "There were 5 findings total.".to_owned();
    let summary = SummaryDocument {
        id: "smith-2019".to_owned(),
        claims: vec![claim.clone()],
        evidence: Some(Evidence {
            sources: BTreeMap::from([(
                DEFAULT_SOURCE.to_owned(),
                SourcePair {
                    markdown: SourceRecord {
                        source: "md/smith-2019/smith-2019.md".to_owned(),
                        sha256: sha256_bytes(fixture.markdown.as_bytes()),
                    },
                    pdf: SourceRecord {
                        source: "pdfs/smith-2019.pdf".to_owned(),
                        sha256: sha256_file(&fixture.corpus.root().join("pdfs/smith-2019.pdf"))
                            .unwrap(),
                    },
                },
            )]),
            claims: vec![ClaimEvidence {
                claim: 0,
                claim_sha256: sha256_bytes(claim.as_bytes()),
                locators: vec![Locator {
                    source: DEFAULT_SOURCE.to_owned(),
                    exact: "finding text".to_owned(),
                    markdown: MarkdownLocator {
                        line: 1,
                        column: 1,
                        unit: UnitKind::Paragraph,
                        section: vec![],
                    },
                    pdf: PdfLocator {
                        page: 1,
                        backend: PdfBackend::MutoolNative,
                    },
                }],
            }],
        }),
    };

    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([(
                (PdfBackend::MutoolNative, 1),
                "The finding text.".to_owned(),
            )]),
        },
    );

    let token_issues: Vec<_> = report
        .issues
        .iter()
        .filter(|i| i.code == "uncovered_token")
        .collect();
    assert!(!token_issues.is_empty());
    assert!(token_issues.iter().all(|i| i.severity == Severity::Warning));
    assert!(report.is_valid(), "warnings should not make report invalid");
}
```

- [ ] **Step 4: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

- [ ] **Step 5: Commit**

```
feat(validate): check required-token coverage per claim

Each claim's required tokens (numbers, number words, quoted phrases)
must appear in at least one locator. Severity is configurable:
coverage.tokens = error (default) | warn | off.
```

---

### Task 7: Weak section warning

When every locator for a claim sits under a weak section, emit `weak_section_only` as a warning.

**Files:**
- Modify: `src/sections.rs`
- Modify: `src/validate.rs`
- Modify: `tests/validate.rs`

- [ ] **Step 1: Add weak-section check to `sections.rs`**

```rust
pub fn is_weak_section(section: &[String], weak_list: &[String]) -> bool {
    if weak_list.is_empty() {
        return false;
    }
    section.iter().any(|heading| {
        let lower = heading.to_lowercase();
        weak_list
            .iter()
            .any(|weak| lower.contains(&weak.to_lowercase()))
    })
}
```

- [ ] **Step 2: Wire into `validate_document`**

After the per-locator loop for a claim entry, check whether all locators with resolved sections sit under a weak section:

```rust
let weak_sections = &corpus.sections_config().weak;
if !weak_sections.is_empty() && !entry.locators.is_empty() {
    let all_weak = entry.locators.iter().all(|loc| {
        let source_name = loc.source.as_str();
        if let Some(units) = source_units.get(source_name) {
            if let Ok(unit) = resolve_unit(
                units,
                loc.markdown.unit,
                loc.markdown.line,
                loc.markdown.column,
            ) {
                sections::is_weak_section(&unit.section, weak_sections)
            } else {
                false
            }
        } else {
            false
        }
    });
    if all_weak {
        issues.push(issue(
            "weak_section_only",
            Severity::Warning,
            format!(
                "every locator for claim {} is under a weak section",
                entry.claim
            ),
            Some(entry.claim),
            None,
        ));
    }
}
```

- [ ] **Step 3: Write weak section tests**

In `tests/validate.rs`:

```rust
#[test]
fn weak_section_only_fires_when_all_locators_are_weak() {
    let fixture = Fixture::with_config(
        "# Limitations\n\nSupported once in the text.\n",
        "cache:\n  root: \".cache/pdf-text\"\nsections:\n  weak:\n    - Limitations\n",
    );
    let mut summary = fixture.summary("Supported once", PdfBackend::MutoolNative);
    summary.evidence.as_mut().unwrap().claims[0].locators[0]
        .markdown
        .line = 3;

    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([((PdfBackend::MutoolNative, 1), "Supported once.".to_owned())]),
        },
    );

    let weak: Vec<_> = report
        .issues
        .iter()
        .filter(|i| i.code == "weak_section_only")
        .collect();
    assert_eq!(weak.len(), 1);
    assert_eq!(weak[0].severity, Severity::Warning);
    assert!(report.is_valid(), "weak_section_only is a warning, not an error");
}

#[test]
fn weak_section_does_not_fire_when_one_locator_is_not_weak() {
    let fixture = Fixture::with_config(
        "# Results\n\nFirst locator.\n\n# Limitations\n\nSecond locator.\n",
        "cache:\n  root: \".cache/pdf-text\"\nsections:\n  weak:\n    - Limitations\n",
    );
    let claim = "A claim with two locators.".to_owned();
    let summary = SummaryDocument {
        id: "smith-2019".to_owned(),
        claims: vec![claim.clone()],
        evidence: Some(Evidence {
            sources: BTreeMap::from([(
                DEFAULT_SOURCE.to_owned(),
                SourcePair {
                    markdown: SourceRecord {
                        source: "md/smith-2019/smith-2019.md".to_owned(),
                        sha256: sha256_bytes(fixture.markdown.as_bytes()),
                    },
                    pdf: SourceRecord {
                        source: "pdfs/smith-2019.pdf".to_owned(),
                        sha256: sha256_file(&fixture.corpus.root().join("pdfs/smith-2019.pdf"))
                            .unwrap(),
                    },
                },
            )]),
            claims: vec![ClaimEvidence {
                claim: 0,
                claim_sha256: sha256_bytes(claim.as_bytes()),
                locators: vec![
                    Locator {
                        source: DEFAULT_SOURCE.to_owned(),
                        exact: "First locator".to_owned(),
                        markdown: MarkdownLocator {
                            line: 3,
                            column: 1,
                            unit: UnitKind::Paragraph,
                            section: vec![],
                        },
                        pdf: PdfLocator {
                            page: 1,
                            backend: PdfBackend::MutoolNative,
                        },
                    },
                    Locator {
                        source: DEFAULT_SOURCE.to_owned(),
                        exact: "Second locator".to_owned(),
                        markdown: MarkdownLocator {
                            line: 7,
                            column: 1,
                            unit: UnitKind::Paragraph,
                            section: vec![],
                        },
                        pdf: PdfLocator {
                            page: 1,
                            backend: PdfBackend::MutoolNative,
                        },
                    },
                ],
            }],
        }),
    };

    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([(
                (PdfBackend::MutoolNative, 1),
                "First locator. Second locator.".to_owned(),
            )]),
        },
    );

    assert!(
        !report.issues.iter().any(|i| i.code == "weak_section_only"),
        "{:?}",
        report.issues
    );
}
```

- [ ] **Step 4: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

- [ ] **Step 5: Commit**

```
feat(validate): weak_section_only warning for claims under weak headings

When every locator for a claim sits under a heading matching
sections.weak, a warning is emitted. The check uses case-insensitive
substring matching against the section path.
```

---

### Task 8: Update `locate` to record section paths

When `locate` emits a new evidence record, it should include the resolved section path in the `markdown` locator.

**Files:**
- Modify: `src/cli.rs:234-346` (the `locate` function)
- Modify: `tests/cli.rs`

- [ ] **Step 1: Include section path in locate output**

In the `locate` function in `src/cli.rs`, after resolving the unit, capture its section path:

```rust
let section = unit.section.clone();
```

And use it in the `MarkdownLocator`:

```rust
markdown: MarkdownLocator {
    line: unit.line,
    column: unit.column,
    unit: unit.kind,
    section,
},
```

- [ ] **Step 2: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

- [ ] **Step 3: Commit**

```
feat(cli): record section paths in locate output

The locate command now includes the resolved section path in the
markdown locator of its output, matching what validate expects.
```

---

### Task 9: Update `validate_document` signature and final wiring

The `validate_document` function currently takes `(corpus, summary, provider)`. It needs the `Corpus` for coverage/sections config, which it already has. Verify the full integration works end-to-end.

**Files:**
- Modify: `tests/cli.rs` (if CLI integration tests need updates)
- No signature change needed — `Corpus` already provides the config accessors.

- [ ] **Step 1: Verify full check suite**

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo build --locked
cargo test --locked
cargo run --locked -- --format text doctor
```

All must pass.

- [ ] **Step 2: Write the handoff**

Update `.handoffs/` with a new file recording Phase 2 completion, decisions made, and what's next (Phase 3).

- [ ] **Step 3: Commit the handoff**

```
docs: add handoff for Phase 2 completion
```

---

## Verification

After all tasks:

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo build --locked
cargo test --locked
cargo run --locked -- --format text doctor
```

Expected: ~95+ tests, all green. Three new error codes: `uncovered_token`, `stale_section`, `weak_section_only`. The `EvidenceIssue` type now carries `severity`, and `ValidationReport::is_valid()` ignores warnings.
