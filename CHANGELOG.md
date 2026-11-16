## [0.1.0] - 2026-11-16

### 🚀 Features

- Scaffold deterministic evidence validator
- *(markdown)* Parse semantic markdown units
- *(evidence)* Model claim evidence records
- *(pdf)* Add native text extraction
- *(pdf)* Cache deterministic page OCR
- *(cli)* Add evidence validation commands
- *(terms)* Support configurable claim vocabulary
- *(config)* Add page_segmentation_mode, expose OcrConfig on Corpus
- *(ocr)* Derive profile from config, remove hardcoded constants
- *(ocr)* Thread OcrConfig through PdfTools and update doctor
- *(ocr)* Honor enabled: false with ocr_disabled error
- *(evidence)* Add named source pairs with bare-form desugaring
- *(config)* Add corpus.sources with bare-form desugaring
- *(corpus)* Add per-source template resolution
- *(validate)* Source-aware validation with per-source file checks
- *(cli)* Source-aware locate with --source flag
- *(evidence)* Add severity field to EvidenceIssue
- *(markdown)* Track section paths through heading stack
- *(validate)* Verify section paths with stale_section error
- *(tokens)* Extract required and advisory tokens from claim text
- *(config)* Add coverage and sections configuration blocks
- *(validate)* Check required-token coverage per claim
- *(validate)* Weak_section_only warning for claims under weak headings
- *(cli)* Record section paths in locate output
- *(propose)* Add data types and sentence splitting
- *(propose)* Deterministic candidate locator generation
- *(cli)* Add propose subcommand with human and JSON output
- *(review)* Add review data types and evidence hashing
- *(evidence)* Add optional review field to SummaryDocument
- *(review)* Structural validation for review entries
- *(validate)* Check review staleness during document validation
- *(cli)* Add --require-review flag to check and audit
- *(cli)* Add schema and completions via librebar parse_with
- *(extract)* Add extract command, dehyphenation, and quote folding
- *(evidence)* Add PDF-only sources and per-claim corroboration legs
- *(coverage)* Coverage.words checks every content word of a claim

### 🐛 Bug Fixes

- Tighten evidence validation
- *(config)* Accept unquoted `coverage.tokens: off`
- Address 11 findings from 2026-08-10 full-repo audit
- Address remaining 5 audit findings; accept 4 as-designed
- *(cache)* Ignore untrusted corpus entries
- *(config)* Contain configured cache roots
- *(corpus)* Contain text source reads
- *(cli)* Bound summary discovery
- *(config)* Bound OCR render DPI
- *(pdf)* Pin external tool executables
- *(pdf)* Reject drifted structured text
- *(doctor)* Enforce tool version ranges
- *(markdown)* Avoid multibyte HTML slicing
- *(cli)* Preflight PDF toolchain
- *(output)* Handle closed stdout cleanly
- *(propose)* Preserve partial batch results
- *(schema)* Unify runtime issue registry
- *(propose)* Stabilize JSON envelope
- *(cli)* Preserve authoring output defaults
- *(config)* Disable machine-wide overlays
- *(corpus)* Expose summary ID grammar
- *(package)* Make the crate non-publishable
- *(evidence)* Type coordinate domains
- *(diagnostics)* Use schema enum names
- *(pdf)* Read MuPDF stext XML instead of stext.json
- *(discover)* Skip hidden entries when scanning for summaries

### 📚 Documentation

- Document the evidence contract
- Multi-source design spec and handoffs
- Add handoff for Phases 0 and 1 completion
- Add Phase 2 implementation plan and handoff
- Add Phase 3 implementation plan and handoff
- Add Phase 4 implementation plan and handoff
- Rewrite README for Phases 0-4 and schema
- Add session handoff for Phase 4, schema, and README
- Add audit remediation handoff
- *(audit)* Record OCR cache trust remediation
- *(audit)* Record cache containment remediation
- *(audit)* Record contained read remediation
- *(audit)* Record bounded summary discovery
- *(audit)* Record OCR DPI remediation
- *(audit)* Record external tool path remediation
- *(audit)* Record toolchain and markdown fixes
- *(audit)* Record failure-mode remediations
- *(audit)* Record contract remediations
- *(audit)* Record output and configuration fixes
- *(audit)* Record type and API fixes
- *(audit)* Record report contract fixes
- *(audit)* Record performance fixes
- *(audit)* Record proposal and allocation fixes
- *(audit)* Record performance and contract fixes
- *(audit)* Record verification gate fixes
- *(audit)* Record policy and regression actions
- *(audit)* Close remaining structural findings
- *(record)* Capture needs before the first release
- *(record)* Record the markdown-leg and paragraph decisions
- Mark the library surface unstable and move the split to 0.2

### ⚡ Performance

- *(validate)* Batch multi-page native extraction
- *(propose)* Prepare Markdown spans once
- *(markdown)* Index locator coordinates
- *(propose)* Bound PDF verification attempts
- *(review)* Consume canonical JSON values
- *(corpus)* Borrow configured source names
- *(sections)* Normalize weak headings once
- *(build)* Enable release cross-module optimization

### 🚜 Refactor

- *(terms)* Encode rename fallibility
- *(propose)* Reuse evidence locator types
- *(propose)* Require verified PDF matches
- *(validate)* Split validation concerns
- *(cli)* Share summary evaluation
- *(evidence)* Centralize issue construction
- *(pdf)* Share subprocess execution
- *(terms)* Remove dead output helpers
- Rename project from receipts to custody

### 🧪 Testing

- *(cli)* Add propose integration test
- *(cli)* Add integration tests for --require-review
- *(reports)* Support typed JSON round trips
- *(cli)* Pin structured output contracts
- *(cli)* Pin nested summary discovery
- *(markdown)* Pin non-origin coordinates

### ⚙️ Miscellaneous Tasks

- Move specs to record/, add Phase 0 implementation plan
- Add Phase 1 implementation plan (named source pairs)
- Update justfile and config to match fleet baseline
- Update gitignore, add first cased audit
- *(audit)* Complete actions-taken ledger — 16 fixed, 4 accepted, 0 open
- Add second full repo audit
- *(gates)* Verify formatting and MSRV
- *(github)* Run the repository verification gate
- Bump dev rust version to 1.98.0
- Add canon cd workflow
- Bump toolchain rust version
- *(release)* Prepare the crate for its first publish
- Run the gate on macOS with brew-installed tools and PDF runtime
- Add gh-coda config and Homebrew formula template
- *(release)* Refresh the tree for the 0.1.0 release
- Add the canon actions and workflows cd.yml depends on

### 💼 Other

- *(deps)* Update all dependencies, migrate to librebar 0.6.0
- *(deps)* Update sha2 0.10 to 0.11
- *(build)* Disable implicit dotenv loading
- *(deps)* Tighten the license allowlist
- Add xtask for man pages and shell completions
