# Phase 1: Named Source Pairs — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a summary cite multiple named Markdown+PDF source pairs instead of exactly one, with backward-compatible desugaring that keeps single-source documents working unchanged.

**Architecture:** Both the evidence schema and the corpus config gain a `sources` map. Bare `markdown`/`pdf` at either level desugars to `sources["default"]` at the `serde_json::Value` boundary before typed structs see it, so no downstream code knows more than one shape exists. Validation gains a source dimension — file checks run once per source, locators reference their source by name.

**Tech Stack:** Rust, serde, serde_json, clap, anyhow, BTreeMap (deterministic iteration)

**Spec:** `record/superpowers/specs/2026-07-30-multi-source-and-support-design.md` — Phase 1 section

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `src/evidence.rs` | Modify | Add `SourcePair`, change `Evidence` to use `sources: BTreeMap`, add `source` to `Locator`, add desugaring function, update `validate_evidence_structure` |
| `src/config.rs` | Modify | Add `SourceTemplates`, change `CorpusLayout` to use `sources: BTreeMap`, load config as `Value` then desugar before deserializing, update template validation |
| `src/corpus.rs` | Modify | Store `sources` from config, add per-source resolution methods, keep single-source convenience methods as delegates to `"default"` |
| `src/validate.rs` | Modify | Validate per source — resolve files, check hashes, parse Markdown once per source, group PDF work by `(source, backend, page)` |
| `src/cli.rs` | Modify | `locate` gains `--source` flag, `LocateResult` includes source name, update source resolution to use corpus source methods |
| `tests/evidence.rs` | Modify | Update for new `Evidence` shape, add desugaring tests |
| `tests/foundation.rs` | Modify | Add config desugaring tests, source template tests |
| `tests/cli.rs` | Modify | Add multi-source locate/check tests |
| `tests/validate.rs` | Modify | Add multi-source validation tests, error code tests |

---

## Design Decisions

**Why Value-level desugaring for config?** `CorpusLayout` uses `#[serde(default)]` which fills in default values for missing fields. This makes it impossible to distinguish "user omitted `markdown`" from "user set `markdown` to its default" at the struct level. Value-level desugaring inspects the raw YAML before defaults apply, so conflict detection (`conflicting_source_form`) is reliable.

**Why `BTreeMap` not `HashMap`?** Deterministic iteration order. Error messages, JSON output, and source validation all iterate the map, and non-deterministic ordering would make test assertions brittle and `--json` output non-reproducible.

**Why `source` defaults to `"default"` on Locator?** Serde's `#[serde(default)]` on the field handles this. Old YAML without `source` gets `"default"`, so desugared single-source documents need no locator changes.

---

### Task 1: Evidence schema — new types, desugaring, and structural validation

This is the foundation. Every other task depends on the Evidence struct shape.

**Files:**
- Modify: `src/evidence.rs`
- Test: `tests/evidence.rs`

- [ ] **Step 1: Write failing test — desugaring bare evidence to sources.default**

Add to `tests/evidence.rs`:

```rust
#[test]
fn desugars_bare_evidence_into_sources_default() {
    let yaml = r#"
id: smith-2019
claims:
  - "Transport remained laminar."
evidence:
  markdown:
    source: "md/smith-2019.md"
    sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  pdf:
    source: "pdfs/smith-2019.pdf"
    sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  claims:
    - claim: 0
      claim_sha256: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
      locators:
        - exact: "no turbulent mixing"
          markdown: {line: 1, column: 1, unit: paragraph}
          pdf: {page: 3, backend: mutool-native}
"#;
    let terms = receipts::terms::Terms::default();
    let doc = receipts::evidence::parse_summary(yaml, &terms).unwrap();
    let evidence = doc.evidence.unwrap();

    assert!(evidence.sources.contains_key("default"));
    assert_eq!(evidence.sources.len(), 1);
    let default = &evidence.sources["default"];
    assert_eq!(default.markdown.source, "md/smith-2019.md");
    assert_eq!(default.pdf.source, "pdfs/smith-2019.pdf");
    assert_eq!(evidence.claims[0].locators[0].source, "default");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --locked --test evidence desugars_bare`
Expected: FAIL — `Evidence` has no `sources` field.

- [ ] **Step 3: Add `SourcePair` struct and update `Evidence`**

In `src/evidence.rs`, add after `SourceRecord`:

```rust
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePair {
    pub markdown: SourceRecord,
    pub pdf: SourceRecord,
}
```

Change `Evidence`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub sources: BTreeMap<String, SourcePair>,
    pub claims: Vec<ClaimEvidence>,
}
```

Add `source` field to `Locator`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Locator {
    #[serde(default = "default_source_name")]
    pub source: String,
    pub exact: String,
    pub markdown: MarkdownLocator,
    pub pdf: PdfLocator,
}

fn default_source_name() -> String {
    "default".to_owned()
}
```

- [ ] **Step 4: Add source name validation**

In `src/evidence.rs`:

```rust
pub fn validate_source_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
}
```

- [ ] **Step 5: Add evidence desugaring function**

In `src/evidence.rs`:

```rust
fn desugar_evidence_sources(value: &mut serde_json::Value) -> Result<()> {
    let Some(evidence) = value.get_mut("evidence") else {
        return Ok(());
    };
    let Some(obj) = evidence.as_object_mut() else {
        return Ok(());
    };

    let has_bare_md = obj.contains_key("markdown");
    let has_bare_pdf = obj.contains_key("pdf");
    let has_sources = obj.contains_key("sources");

    if (has_bare_md || has_bare_pdf) && has_sources {
        bail!("evidence has both bare markdown/pdf and sources");
    }

    if has_bare_md || has_bare_pdf {
        let mut default_source = serde_json::Map::new();
        if let Some(md) = obj.remove("markdown") {
            default_source.insert("markdown".to_owned(), md);
        }
        if let Some(pdf) = obj.remove("pdf") {
            default_source.insert("pdf".to_owned(), pdf);
        }
        let mut sources = serde_json::Map::new();
        sources.insert("default".to_owned(), serde_json::Value::Object(default_source));
        obj.insert("sources".to_owned(), serde_json::Value::Object(sources));
    }

    // Default source on locators without one
    if let Some(claims) = obj.get_mut("claims").and_then(|v| v.as_array_mut()) {
        for entry in claims {
            if let Some(locators) = entry.get_mut("locators").and_then(|v| v.as_array_mut()) {
                for locator in locators {
                    if let Some(obj) = locator.as_object_mut() {
                        obj.entry("source").or_insert_with(|| serde_json::Value::String("default".to_owned()));
                    }
                }
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 6: Call desugaring in `parse_summary`**

Update `parse_summary`:

```rust
pub fn parse_summary(content: &str, terms: &Terms) -> Result<SummaryDocument> {
    let mut value =
        librebar::config::parse_yaml(content).context("failed to parse summary YAML")?;
    terms.canonicalize(&mut value)?;
    desugar_evidence_sources(&mut value)?;
    serde_json::from_value(value).context("failed to decode summary evidence")
}
```

- [ ] **Step 7: Update `validate_evidence_structure`**

Replace the current method body to iterate `sources`:

```rust
pub fn validate_evidence_structure(&self, terms: &Terms) -> Vec<EvidenceIssue> {
    let Some(evidence) = self.evidence.as_ref() else {
        return vec![issue("missing_evidence", "summary is missing evidence section", None, None)];
    };
    let mut issues = Vec::new();

    if evidence.sources.is_empty() {
        issues.push(issue("empty_sources", "evidence sources map is empty", None, None));
    }

    for (name, pair) in &evidence.sources {
        if !validate_source_name(name) {
            issues.push(issue("invalid_source_name", format!("source name {name:?} is not a valid path component"), None, None));
        }
        validate_source(&format!("{name}/markdown"), &pair.markdown, &mut issues);
        validate_source(&format!("{name}/pdf"), &pair.pdf, &mut issues);
    }

    // Track which sources are actually referenced by locators
    let mut referenced_sources: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();

    let mut counts = vec![0_usize; self.claims.len()];
    for entry in &evidence.claims {
        // ... existing claim range, hash, empty_locators checks (unchanged) ...

        for (locator_index, locator) in entry.locators.iter().enumerate() {
            if !evidence.sources.contains_key(&locator.source) {
                issues.push(issue(
                    "unknown_source",
                    format!("locator references undeclared source {:?}", locator.source),
                    Some(entry.claim),
                    Some(locator_index),
                ));
            } else {
                referenced_sources.insert(&locator.source);
            }
            validate_locator(entry.claim, locator_index, locator, &mut issues);
        }
    }

    // ... existing counts check (missing_evidence_entry, duplicate_entry) ...

    // Check for unused sources
    for name in evidence.sources.keys() {
        if !referenced_sources.contains(name.as_str()) {
            issues.push(issue("unused_source", format!("source {name:?} is declared but not referenced by any locator"), None, None));
        }
    }

    issues
}
```

Keep the existing checks for claim range, stale hash, empty locators, counts, duplicate entries. Just add the source-related checks around them.

- [ ] **Step 8: Write additional tests**

Add to `tests/evidence.rs`:

```rust
#[test]
fn rejects_conflicting_source_form() {
    let yaml = r#"
id: test
claims: ["one"]
evidence:
  markdown: {source: "a.md", sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}
  pdf: {source: "a.pdf", sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}
  sources:
    default:
      markdown: {source: "a.md", sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}
      pdf: {source: "a.pdf", sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}
  claims: []
"#;
    let terms = receipts::terms::Terms::default();
    assert!(receipts::evidence::parse_summary(yaml, &terms).is_err());
}

#[test]
fn parses_explicit_named_sources() {
    let yaml = r#"
id: test
claims: ["one"]
evidence:
  sources:
    default:
      markdown: {source: "a.md", sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}
      pdf: {source: "a.pdf", sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}
    supplement:
      markdown: {source: "s.md", sha256: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}
      pdf: {source: "s.pdf", sha256: "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"}
  claims:
    - claim: 0
      claim_sha256: "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
      locators:
        - source: supplement
          exact: "some text"
          markdown: {line: 1, column: 1, unit: paragraph}
          pdf: {page: 1, backend: mutool-native}
"#;
    let terms = receipts::terms::Terms::default();
    let doc = receipts::evidence::parse_summary(yaml, &terms).unwrap();
    let evidence = doc.evidence.unwrap();
    assert_eq!(evidence.sources.len(), 2);
    assert!(evidence.sources.contains_key("supplement"));
    assert_eq!(evidence.claims[0].locators[0].source, "supplement");
}

#[test]
fn reports_unknown_and_unused_sources() {
    // Build a SummaryDocument programmatically with evidence that has
    // a source "main" declared but locator referencing "other" (unknown)
    // and "main" unreferenced (unused).
    // Assert both error codes appear.
}
```

For the `reports_unknown_and_unused_sources` test: construct the typed structs directly (don't go through YAML) and call `validate_evidence_structure`. Build an Evidence with `sources: {"main": ...}` and a locator with `source: "other"`. Check for both `unknown_source` and `unused_source` in the returned issues.

- [ ] **Step 9: Fix compile errors in `validate.rs` and `cli.rs`**

After changing `Evidence`, code that accesses `evidence.markdown` and `evidence.pdf` won't compile. Apply minimal fixes:

In `src/validate.rs`, replace `evidence.markdown` and `evidence.pdf` with `evidence.sources["default"]` access. Specifically:
- Lines 68-75: change `&evidence.markdown.source` → `&evidence.sources["default"].markdown.source` (and same for pdf)
- Lines 84-90: same pattern for sha256 fields

In `src/cli.rs`, the `LocateResult` struct currently has bare `markdown`/`pdf`. For now, keep it — `locate` always produces a single-source result. The `SourceRecord` it builds comes from the corpus, not from Evidence.

- [ ] **Step 10: Update existing tests to match new Evidence shape**

Any tests that construct `Evidence` directly or assert on its fields need updating. Check `tests/evidence.rs` and `tests/validate.rs` for direct field access.

- [ ] **Step 11: Run full suite and commit**

Run: `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`

Commit: `feat(evidence): add named source pairs with bare-form desugaring`

---

### Task 2: Config schema — add `corpus.sources` with desugaring

**Files:**
- Modify: `src/config.rs`
- Test: `tests/foundation.rs`

- [ ] **Step 1: Write failing test — config desugaring**

Add to `tests/foundation.rs`:

```rust
#[test]
fn desugars_bare_corpus_layout_into_sources_default() {
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        "cache:\n  root: \"c\"\ncorpus:\n  markdown:\n    - \"docs/{id}.md\"\n  pdf: \"files/{id}.pdf\"\n",
    );
    let corpus = Corpus::discover_from(dir.path(), None).unwrap();

    let sources = corpus.source_names();
    assert_eq!(sources, vec!["default"]);
}

#[test]
fn parses_explicit_corpus_sources() {
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        concat!(
            "cache:\n  root: \"c\"\ncorpus:\n",
            "  sources:\n",
            "    default:\n",
            "      markdown:\n        - \"md/{id}.md\"\n",
            "      pdf: \"pdfs/{id}.pdf\"\n",
            "    supplement:\n",
            "      markdown:\n        - \"md/{id}-supp.md\"\n",
            "      pdf: \"pdfs/{id}-supp.pdf\"\n",
        ),
    );
    let corpus = Corpus::discover_from(dir.path(), None).unwrap();

    let mut sources = corpus.source_names();
    sources.sort();
    assert_eq!(sources, vec!["default", "supplement"]);
}

#[test]
fn rejects_conflicting_corpus_source_form() {
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        concat!(
            "cache:\n  root: \"c\"\ncorpus:\n",
            "  markdown:\n    - \"md/{id}.md\"\n",
            "  pdf: \"pdfs/{id}.pdf\"\n",
            "  sources:\n    default:\n      markdown:\n        - \"x/{id}.md\"\n      pdf: \"x/{id}.pdf\"\n",
        ),
    );
    assert!(Corpus::discover_from(dir.path(), None).is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Add `SourceTemplates` struct**

In `src/config.rs`:

```rust
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceTemplates {
    pub markdown: Vec<String>,
    pub pdf: String,
}
```

- [ ] **Step 4: Change `CorpusLayout` to use `sources`**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CorpusLayout {
    pub summaries: String,
    pub sources: BTreeMap<String, SourceTemplates>,
}

impl Default for CorpusLayout {
    fn default() -> Self {
        let mut sources = BTreeMap::new();
        sources.insert(
            "default".to_owned(),
            SourceTemplates {
                markdown: vec!["md/{id}.md".to_owned(), "md/{id}/{id}.md".to_owned()],
                pdf: "pdfs/{id}.pdf".to_owned(),
            },
        );
        Self {
            summaries: "summaries/{id}.yaml".to_owned(),
            sources,
        }
    }
}
```

- [ ] **Step 5: Change config loading to Value-level desugaring**

Update `config::load()`:

```rust
pub fn load(start: &Path, explicit: Option<&Path>) -> Result<Discovered> {
    let search = to_utf8(start)?;
    let mut loader = librebar::config::ConfigLoader::new("receipts").with_project_search(&search);
    if let Some(path) = explicit {
        loader = loader.with_file(to_utf8(path)?);
    }
    let (mut raw, sources): (serde_json::Value, _) = loader
        .load()
        .map_err(|error| anyhow!("failed to load receipts configuration: {error}"))?;
    desugar_corpus_layout(&mut raw)?;
    let config: Config = serde_json::from_value(raw)
        .context("failed to deserialize receipts configuration")?;
    validate(&config)?;
    let root = resolve_root(sources.project_file.as_deref(), start)?;
    let config_file = sources
        .project_file
        .as_deref()
        .map(|path| PathBuf::from(path.as_str()));
    Ok(Discovered { config, root, config_file })
}
```

Add the desugaring function:

```rust
fn desugar_corpus_layout(value: &mut serde_json::Value) -> Result<()> {
    let Some(corpus) = value
        .get_mut("corpus")
        .and_then(|v| v.as_object_mut())
    else {
        return Ok(());
    };

    let has_bare_md = corpus.contains_key("markdown");
    let has_bare_pdf = corpus.contains_key("pdf");
    let has_sources = corpus.contains_key("sources");

    if (has_bare_md || has_bare_pdf) && has_sources {
        bail!("corpus config has both bare markdown/pdf and sources");
    }

    if has_bare_md || has_bare_pdf {
        let mut default_source = serde_json::Map::new();
        if let Some(md) = corpus.remove("markdown") {
            default_source.insert("markdown".to_owned(), md);
        }
        if let Some(pdf) = corpus.remove("pdf") {
            default_source.insert("pdf".to_owned(), pdf);
        }
        let mut sources = serde_json::Map::new();
        sources.insert("default".to_owned(), serde_json::Value::Object(default_source));
        corpus.insert("sources".to_owned(), serde_json::Value::Object(sources));
    }

    Ok(())
}
```

- [ ] **Step 6: Update template validation**

Replace the current `validate()` to iterate `config.corpus.sources`:

```rust
fn validate(config: &Config) -> Result<()> {
    if config.corpus.sources.is_empty() {
        bail!("corpus must have at least one source");
    }
    for (name, templates) in &config.corpus.sources {
        if !crate::evidence::validate_source_name(name) {
            bail!("corpus source name {name:?} is not a valid path component");
        }
        if templates.markdown.is_empty() {
            bail!("corpus source {name:?} must list at least one markdown template");
        }
        let mut all = vec![&templates.pdf];
        all.extend(&templates.markdown);
        for template in all {
            if !template.contains("{id}") {
                bail!("corpus template {template:?} must contain {{id}}");
            }
            if Path::new(template.as_str()).is_absolute() {
                bail!("corpus template {template:?} must be relative to the corpus root");
            }
            if template.split('/').any(|segment| segment == "..") {
                bail!("corpus template {template:?} must not contain ..");
            }
        }
    }
    // ... keep existing ocr and terms validation unchanged ...
}
```

- [ ] **Step 7: Run tests and commit**

Run: `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`

Commit: `feat(config): add corpus.sources with bare-form desugaring`

---

### Task 3: Corpus resolution per source

**Files:**
- Modify: `src/corpus.rs`
- Test: `tests/foundation.rs`

- [ ] **Step 1: Write failing tests for per-source resolution**

Add to `tests/foundation.rs`:

```rust
#[test]
fn resolves_source_specific_paths() {
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        concat!(
            "cache:\n  root: \"c\"\ncorpus:\n",
            "  sources:\n",
            "    default:\n",
            "      markdown:\n        - \"md/{id}.md\"\n",
            "      pdf: \"pdfs/{id}.pdf\"\n",
            "    supplement:\n",
            "      markdown:\n        - \"md/{id}-supp.md\"\n",
            "      pdf: \"pdfs/{id}-supp.pdf\"\n",
        ),
    );
    let corpus = Corpus::discover_from(dir.path(), None).unwrap();
    let root = corpus.root().to_path_buf();

    assert_eq!(
        corpus.markdown_candidates_for("smith-2019", "default").unwrap(),
        vec![root.join("md/smith-2019.md")]
    );
    assert_eq!(
        corpus.pdf_path_for("smith-2019", "supplement").unwrap(),
        root.join("pdfs/smith-2019-supp.pdf")
    );
    assert!(corpus.pdf_path_for("smith-2019", "unknown").is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

- [ ] **Step 3: Update `Corpus` for source-aware resolution**

The `Corpus` struct currently stores `layout: CorpusLayout`. After Task 2, `CorpusLayout` has `sources: BTreeMap<String, SourceTemplates>`. Add methods:

```rust
pub fn source_names(&self) -> Vec<String> {
    self.layout.sources.keys().cloned().collect()
}

pub fn markdown_candidates_for(&self, id: &str, source: &str) -> Result<Vec<PathBuf>> {
    validate_id(id)?;
    let templates = self.layout.sources.get(source)
        .with_context(|| format!("unknown source template {source:?}"))?;
    Ok(templates.markdown.iter()
        .map(|template| self.root.join(render(template, id)))
        .collect())
}

pub fn pdf_path_for(&self, id: &str, source: &str) -> Result<PathBuf> {
    validate_id(id)?;
    let templates = self.layout.sources.get(source)
        .with_context(|| format!("unknown source template {source:?}"))?;
    Ok(self.root.join(render(&templates.pdf, id)))
}
```

Keep existing `markdown_candidates` and `pdf_path` as delegates to `"default"`:

```rust
pub fn markdown_candidates(&self, id: &str) -> Result<Vec<PathBuf>> {
    self.markdown_candidates_for(id, "default")
}

pub fn pdf_path(&self, id: &str) -> Result<PathBuf> {
    self.pdf_path_for(id, "default")
}
```

This keeps all existing call sites working unchanged.

- [ ] **Step 4: Run tests and commit**

Commit: `feat(corpus): add per-source template resolution`

---

### Task 4: Source-aware validation

**Files:**
- Modify: `src/validate.rs`
- Test: `tests/validate.rs`

- [ ] **Step 1: Write failing test for multi-source validation**

Add a test in `tests/validate.rs` that creates a corpus with two sources, a summary with two source pairs and locators referencing different sources, and asserts validation succeeds.

Also add tests for:
- `unknown_source_template` — evidence declares a source not in the corpus config
- `unused_source` — evidence declares a source no locator references (if not already tested in Task 1)

- [ ] **Step 2: Rewrite `validate_document` for source-aware validation**

The current function validates one markdown and one pdf. Rewrite to iterate `evidence.sources`:

```rust
pub fn validate_document(
    corpus: &Corpus,
    summary: &SummaryDocument,
    provider: &impl PdfTextProvider,
) -> ValidationReport {
    let mut issues = summary.validate_evidence_structure(corpus.terms());
    let Some(evidence) = summary.evidence.as_ref() else {
        return ValidationReport { id: summary.id.clone(), issues };
    };

    // Per-source: resolve paths, validate containment, hash, parse markdown
    let mut source_markdown: BTreeMap<String, Vec<MarkdownUnit>> = BTreeMap::new();
    let mut source_pdf_sha: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut source_pdf_path: BTreeMap<String, PathBuf> = BTreeMap::new();

    for (source_name, pair) in &evidence.sources {
        // Resolve templates — unknown_source_template
        let md_candidates = match corpus.markdown_candidates_for(&summary.id, source_name) {
            Ok(paths) => paths,
            Err(_) => {
                issues.push(issue("unknown_source_template",
                    format!("no configured templates for source {source_name:?}"), None, None));
                continue;
            }
        };
        let md_path = md_candidates.iter().find(|p| p.is_file()).unwrap_or(&md_candidates[0]);
        let pdf_path = match corpus.pdf_path_for(&summary.id, source_name) {
            Ok(path) => path,
            Err(_) => {
                issues.push(issue("unknown_source_template",
                    format!("no configured templates for source {source_name:?}"), None, None));
                continue;
            }
        };

        // Validate paths, containment, hashes (same pattern as current code)
        validate_source_path(corpus, &format!("{source_name}/markdown"), &pair.markdown.source, md_path, &mut issues);
        validate_source_path(corpus, &format!("{source_name}/pdf"), &pair.pdf.source, &pdf_path, &mut issues);
        let md_safe = validate_resolved_source(corpus, &format!("{source_name}/markdown"), md_path, &mut issues);
        let pdf_safe = validate_resolved_source(corpus, &format!("{source_name}/pdf"), &pdf_path, &mut issues);

        if md_safe && pdf_safe {
            validate_file_hash(&format!("{source_name}/markdown"), md_path, &pair.markdown.sha256, &mut issues);
            let pdf_sha = validate_file_hash(&format!("{source_name}/pdf"), &pdf_path, &pair.pdf.sha256, &mut issues);
            source_pdf_sha.insert(source_name.clone(), pdf_sha);
        }
        source_pdf_path.insert(source_name.clone(), pdf_path);

        // Parse markdown once per source
        if let Ok(md_source) = fs::read_to_string(md_path) {
            source_markdown.insert(source_name.clone(), parse_units(&md_source));
        }
    }

    // Locator validation — group PDF work by (source, backend, page)
    let mut pdf_pages: HashMap<(String, PdfBackend, usize), Result<String, String>> = HashMap::new();

    for entry in &evidence.claims {
        for (locator_index, locator) in entry.locators.iter().enumerate() {
            let source_name = &locator.source;

            // Markdown check against the source's parsed units
            if let Some(units) = source_markdown.get(source_name) {
                // ... same unit resolution + exact_count logic as current, using source's units
            }

            // OCR disabled check (existing from Phase 0)
            if locator.pdf.backend == PdfBackend::TesseractOcr && !corpus.ocr_config().enabled {
                // ... existing ocr_disabled logic ...
                continue;
            }

            // PDF check against the source's PDF
            if let Some(pdf_path) = source_pdf_path.get(source_name) {
                let pdf_sha = source_pdf_sha.get(source_name)
                    .and_then(|s| s.as_deref())
                    .unwrap_or_default();
                let key = (source_name.clone(), locator.pdf.backend, locator.pdf.page);
                let extracted = pdf_pages.entry(key).or_insert_with(|| {
                    extract_pdf_page(provider, pdf_path, pdf_sha, locator.pdf.backend, locator.pdf.page)
                });
                // ... same exact_count logic as current ...
            }
        }
    }

    ValidationReport { id: summary.id.clone(), issues }
}
```

This is the biggest change in the plan. The structure preserves the existing validation logic but adds the source dimension. Read the current `validate_document` carefully and restructure around sources — don't rewrite from scratch.

- [ ] **Step 3: Run tests and commit**

Commit: `feat(validate): source-aware validation with per-source file checks`

---

### Task 5: CLI updates and integration tests

**Files:**
- Modify: `src/cli.rs`
- Modify: `tests/cli.rs`

- [ ] **Step 1: Add `--source` flag to `locate`**

In `LocateArgs`:

```rust
#[derive(Debug, Args)]
struct LocateArgs {
    id: String,
    #[arg(long)]
    claim: usize,
    #[arg(long)]
    exact: String,
    #[arg(long, default_value = "default")]
    source: String,
    #[arg(long)]
    page: Option<usize>,
    #[arg(long)]
    line: Option<usize>,
    #[arg(long)]
    column: Option<usize>,
}
```

- [ ] **Step 2: Update `locate()` to use source-aware resolution**

Replace `resolve_markdown(corpus, &args.id)?` with `resolve_markdown_for(corpus, &args.id, &args.source)?` and similarly for `corpus.pdf_path`.

Add helper:

```rust
fn resolve_markdown_for(corpus: &Corpus, id: &str, source: &str) -> Result<PathBuf> {
    let candidates = corpus.markdown_candidates_for(id, source)?;
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .with_context(|| format!("no canonical Markdown source found for {id} (source: {source})"))
}
```

Keep `resolve_markdown` as a delegate to `resolve_markdown_for(..., "default")`.

Update the `LocateResult` to include `source`:

```rust
#[derive(Debug, Serialize)]
struct LocateResult {
    source: String,
    markdown: SourceRecord,
    pdf: SourceRecord,
    claim: ClaimEvidence,
    pdf_match: Option<PdfMatchDiagnostic>,
}
```

And the Locator construction to include source:

```rust
Locator {
    source: args.source.clone(),
    exact,
    markdown: MarkdownLocator { ... },
    pdf: PdfLocator { ... },
}
```

- [ ] **Step 3: Write integration tests**

Add to `tests/cli.rs`:

```rust
#[test]
fn check_validates_multi_source_document() {
    // Create a corpus with two sources, a summary referencing both,
    // and verify check passes/fails appropriately.
}
```

The test needs:
- Config with `corpus.sources.default` and `corpus.sources.supplement`
- A summary YAML with `evidence.sources` containing both
- Source files (markdown + pdf) for both sources
- Locators referencing both sources

- [ ] **Step 4: Verify existing single-source tests still pass**

All existing tests use the bare single-source format. Desugaring must keep them working. Run the full suite.

- [ ] **Step 5: Run full CI suite and commit**

Run: `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`

Commit: `feat(cli): source-aware locate with --source flag`

---

## Summary of New Error Codes

| Code | Where | Trigger |
|---|---|---|
| `empty_sources` | `validate_evidence_structure` | evidence.sources is present but empty |
| `unknown_source` | `validate_evidence_structure` | locator references undeclared source |
| `unused_source` | `validate_evidence_structure` | declared source not referenced by any locator |
| `conflicting_source_form` | `desugar_evidence_sources` / `desugar_corpus_layout` | both bare and sources present |
| `unknown_source_template` | `validate_document` | evidence declares a source with no corpus config templates |
| `invalid_source_name` | `validate_evidence_structure` | source name not a safe path component |
