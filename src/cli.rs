use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use librebar::cli::{
    CommandExample, CommandMetadata, ErrorMetadata, OutputField, ResolvedOutputFormat,
    SchemaMetadata, Stability,
};
use serde::Serialize;

use crate::{
    coordinate::{ClaimIndex, Column, Line, Page},
    corpus::Corpus,
    evidence::{
        ClaimEvidence, DEFAULT_SOURCE, EvidenceIssue, IssueCode, Locator, MarkdownLocator,
        PdfBackend, PdfLocator, Severity, SourceRecord, issue, issue_code, parse_summary,
    },
    hash::sha256_file,
    markdown::{exact_count, parse_units},
    normalize::normalize,
    output::{print_json, print_line},
    pdf::{PdfBbox, PdfTextProvider, PdfTools, cache::CacheReadPolicy, matching_bbox},
    terms::Terms,
    validate::{ValidationReport, validate_document},
};

#[derive(Debug, Parser)]
// `long_about` is set explicitly: without it, clap falls back to the doc comment
// of the flattened `CommonArgs` struct and `--help` describes librebar instead
// of this tool.
#[command(
    name = "receipts",
    version,
    about = "Prove that summary claims are present in their Markdown and PDF sources",
    long_about = "Prove that summary claims are present in their Markdown and PDF sources.

Each claim in a summary document is bound to a literal string that must occur
exactly once inside one Markdown semantic unit and exactly once on one physical
PDF page, after normalization: Unicode whitespace runs collapse to one space,
and by configuration line-end hyphenation is rejoined and typographic quotes
fold to ASCII. Ambiguity is an error, not an occurrence to choose from.

Corpus layout is declared in receipts.yaml, discovered by walking up from the
working directory."
)]
pub struct Cli {
    #[command(flatten)]
    common: librebar::cli::CommonArgs,
    /// Read OCR entries from a cache inside the corpus root.
    #[arg(long, global = true)]
    trust_cache: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Probe corpus paths and external PDF tools.
    Doctor,
    /// Generate one YAML-ready exact evidence record.
    Locate(LocateArgs),
    /// Validate selected summaries, or every evidence-bearing summary.
    Check(CheckArgs),
    /// Inventory valid, missing, and invalid evidence records.
    Audit(AuditArgs),
    /// Suggest candidate locators for claims lacking evidence.
    Propose(ProposeArgs),
    /// Emit Markdown from a PDF's native text layer, one paragraph per block.
    Extract(ExtractArgs),
}

#[derive(Debug, Args)]
struct ExtractArgs {
    id: String,
    /// Source whose PDF template locates the input and whose first Markdown
    /// template is the destination for --write.
    #[arg(long, default_value = DEFAULT_SOURCE)]
    source: String,
    /// Write to the source's Markdown path instead of stdout.
    #[arg(long)]
    write: bool,
    /// Replace an existing Markdown file.
    #[arg(long, requires = "write")]
    force: bool,
}

#[derive(Debug, Serialize)]
struct ExtractReport {
    id: String,
    source: String,
    path: String,
    pages: usize,
    paragraphs: usize,
    sha256: String,
}

#[derive(Debug, Args)]
struct LocateArgs {
    id: String,
    #[arg(long)]
    claim: ClaimIndex,
    #[arg(long)]
    exact: String,
    #[arg(long, default_value = DEFAULT_SOURCE)]
    source: String,
    #[arg(long)]
    page: Option<Page>,
    #[arg(long)]
    line: Option<Line>,
    #[arg(long)]
    column: Option<Column>,
}

#[derive(Debug, Args)]
struct CheckArgs {
    ids: Vec<String>,
    /// Require review entries with supported verdicts for all claims.
    #[arg(long)]
    require_review: bool,
}

#[derive(Debug, Args)]
struct AuditArgs {
    /// Treat summaries without evidence as invalid.
    #[arg(long)]
    strict: bool,
    /// Require review entries with supported verdicts for all claims.
    #[arg(long)]
    require_review: bool,
    ids: Vec<String>,
}

#[derive(Debug, Args)]
struct ProposeArgs {
    ids: Vec<String>,
    /// Process all claims, not just those without evidence.
    #[arg(long)]
    all: bool,
    /// Maximum candidates per claim.
    #[arg(long, default_value = "3")]
    candidates: usize,
}

#[derive(Debug, Serialize)]
struct CheckReport {
    valid: usize,
    skipped: usize,
    invalid: usize,
    summaries: Vec<ValidationReport>,
}

#[derive(Debug, Serialize)]
struct AuditReport {
    valid: usize,
    missing: usize,
    invalid: usize,
    summaries: Vec<AuditSummary>,
}

#[derive(Debug, Serialize)]
struct AuditSummary {
    id: String,
    status: AuditStatus,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    issues: Vec<crate::evidence::EvidenceIssue>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum AuditStatus {
    Valid,
    Missing,
    Invalid,
}

enum SummaryOutcome {
    ParseFailed(EvidenceIssue),
    IdMismatch(EvidenceIssue),
    NoEvidence,
    Validated(ValidationReport),
}

#[derive(Clone, Copy)]
enum MissingEvidencePolicy {
    Validate,
    Skip,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ProposalSummary {
    Proposed(crate::propose::ProposalReport),
    Failed(ValidationReport),
}

#[derive(Debug, Serialize)]
struct LocateResult {
    source: String,
    markdown: SourceRecord,
    pdf: SourceRecord,
    claim: ClaimEvidence,
    pdf_match: Option<PdfMatchDiagnostic>,
}

#[derive(Debug, Serialize)]
struct PdfMatchDiagnostic {
    page: Page,
    backend: PdfBackend,
    #[serde(skip_serializing_if = "Option::is_none")]
    bbox: Option<PdfBbox>,
    #[serde(skip_serializing_if = "Option::is_none")]
    geometry_note: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mean_confidence: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolStatus {
    Ok,
    Missing,
    Unsupported,
}

impl ToolStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Missing => "missing",
            Self::Unsupported => "unsupported",
        }
    }
}

pub fn run() -> Result<()> {
    let cli: Cli = librebar::cli::parse_with(schema_metadata());
    if cli
        .common
        .apply(env!("CARGO_PKG_VERSION"))
        .context("failed to apply common CLI arguments")?
        .is_exit()
    {
        return Ok(());
    }
    let config_path = cli
        .common
        .config_path()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    let explicit = config_path.as_ref().map(|p| Path::new(p.as_str()));
    let corpus = Corpus::discover_from(&cwd, explicit)?;
    crate::normalize::configure(corpus.normalize_config().options());
    let format = if matches!(&cli.command, Command::Locate(_) | Command::Propose(_)) {
        cli.common.output_format_for(true)
    } else {
        cli.common.output_format()
    };
    let json = format == ResolvedOutputFormat::Json;
    let quiet = cli.common.quiet;
    let read_policy = cache_read_policy(corpus.cache_is_corpus_local(), cli.trust_cache);
    let tools = PdfTools::from_config(
        corpus.cache_root().to_path_buf(),
        corpus.pdf_config(),
        read_policy,
    )?;
    match cli.command {
        Command::Doctor => doctor(&corpus, &tools, json, quiet),
        Command::Locate(args) => {
            preflight_toolchain(&tools)?;
            locate(&corpus, &tools, &args, json)
        }
        Command::Check(args) => {
            preflight_toolchain(&tools)?;
            check(&corpus, &tools, &args.ids, json, quiet, args.require_review)
        }
        Command::Audit(args) => {
            preflight_toolchain(&tools)?;
            audit(
                &corpus,
                &tools,
                args.strict,
                args.require_review,
                &args.ids,
                json,
                quiet,
            )
        }
        Command::Propose(args) => {
            preflight_toolchain(&tools)?;
            propose_cmd(&corpus, &tools, &args, json, quiet)
        }
        Command::Extract(args) => {
            preflight_toolchain(&tools)?;
            extract(&corpus, &tools, &args, json, quiet)
        }
    }
}

/// Render a PDF's native text as Markdown: YAML frontmatter, then one
/// `## Page N` heading per physical page and one paragraph per `MuPDF` text
/// block, each already normalized. Every page gets a heading, even an empty
/// one, so the section path always names the physical page.
fn extract(
    corpus: &Corpus,
    tools: &PdfTools,
    args: &ExtractArgs,
    json: bool,
    quiet: bool,
) -> Result<()> {
    let pdf_path = corpus.pdf_path_for(&args.id, &args.source)?;
    if !pdf_path.is_file() {
        bail!("canonical PDF is missing: {}", pdf_path.display());
    }
    let pdf_sha256 = sha256_file(&pdf_path)?;
    let pages = tools.mutool.native_blocks(&pdf_path)?;
    let mutool_version = tools.mutool.version()?;
    let options = corpus.normalize_config().options();

    let mut markdown = String::new();
    // Writing to a String cannot fail; the `let _ =` acknowledges the Result.
    let _ = writeln!(markdown, "---");
    let _ = writeln!(markdown, "id: {}", args.id);
    let _ = writeln!(markdown, "source_format: pdf-native");
    let _ = writeln!(markdown, "source: {}", relative(corpus, &pdf_path));
    let _ = writeln!(markdown, "source_sha256: {pdf_sha256}");
    let _ = writeln!(
        markdown,
        "extractor: receipts {} ({}, {mutool_version})",
        env!("CARGO_PKG_VERSION"),
        crate::pdf::mutool::PROFILE_NAME
    );
    let _ = writeln!(markdown, "normalize: \"{}\"", options.describe());
    let _ = writeln!(markdown, "---");
    let mut paragraphs = 0;
    for (index, blocks) in pages.iter().enumerate() {
        let _ = write!(markdown, "\n## Page {}\n", index + 1);
        for block in blocks {
            markdown.push('\n');
            markdown.push_str(block);
            markdown.push('\n');
            paragraphs += 1;
        }
    }

    if !args.write {
        print_line(format_args!("{}", markdown.trim_end()))?;
        return Ok(());
    }

    let destination = corpus
        .markdown_candidates_for(&args.id, &args.source)?
        .into_iter()
        .next()
        .with_context(|| format!("source {:?} declares no Markdown template", args.source))?;
    if destination.exists() && !args.force {
        bail!(
            "{} already exists; pass --force to replace it",
            destination.display()
        );
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let staging = destination.with_extension(format!("md.{}.tmp", std::process::id()));
    fs::write(&staging, &markdown)
        .with_context(|| format!("failed to write {}", staging.display()))?;
    fs::rename(&staging, &destination).with_context(|| {
        let _ = fs::remove_file(&staging);
        format!("failed to move extraction into {}", destination.display())
    })?;

    let report = ExtractReport {
        id: args.id.clone(),
        source: args.source.clone(),
        path: relative(corpus, &destination),
        pages: pages.len(),
        paragraphs,
        sha256: crate::hash::sha256_bytes(markdown.as_bytes()),
    };
    if json {
        print_json(&report)
    } else if quiet {
        Ok(())
    } else {
        print_line(format_args!(
            "wrote {} ({} pages, {} paragraphs)",
            report.path, report.pages, report.paragraphs
        ))
    }
}

fn preflight_toolchain(tools: &PdfTools) -> Result<()> {
    tools
        .validate_toolchain()
        .context("PDF toolchain preflight failed")
}

const fn cache_read_policy(cache_is_corpus_local: bool, trust_cache: bool) -> CacheReadPolicy {
    if cache_is_corpus_local && !trust_cache {
        CacheReadPolicy::WriteOnly
    } else {
        CacheReadPolicy::Trusted
    }
}

fn schema_metadata() -> SchemaMetadata {
    let mut metadata = SchemaMetadata::new()
        .version(env!("CARGO_PKG_VERSION").to_owned())
        .command(
            "doctor",
            CommandMetadata::new()
                .mutating(false)
                .stability(Stability::Stable)
                .output_field(
                    OutputField::new("corpus", "string").description("Resolved corpus root path"),
                )
                .output_field(
                    OutputField::new("config", "string")
                        .description("Config file path or 'defaults'"),
                )
                .output_field(
                    OutputField::new("mutool", "string").description("MuPDF version or error"),
                )
                .output_field(
                    OutputField::new("tesseract", "string")
                        .description("Tesseract version or error"),
                )
                .output_field(
                    OutputField::new("native_profile", "string")
                        .description("Native PDF extraction profile name"),
                )
                .output_field(
                    OutputField::new("ocr_profile", "string")
                        .description("OCR extraction profile name"),
                )
                .output_field(
                    OutputField::new("normalize", "string")
                        .description("Active text normalization rules"),
                )
                .output_field(OutputField::new("cache", "string").description("Cache root path")),
        )
        .command(
            "extract",
            CommandMetadata::new()
                .mutating(true)
                .stability(Stability::Stable)
                .output_field(
                    OutputField::new("id", "string").description("Summary ID that was extracted"),
                )
                .output_field(
                    OutputField::new("source", "string")
                        .description("Source whose templates were used"),
                )
                .output_field(
                    OutputField::new("path", "string")
                        .description("Corpus-relative Markdown path written (--write only)"),
                )
                .output_field(
                    OutputField::new("pages", "integer").description("Physical pages extracted"),
                )
                .output_field(
                    OutputField::new("paragraphs", "integer")
                        .description("Non-empty text blocks emitted"),
                )
                .output_field(
                    OutputField::new("sha256", "string")
                        .description("SHA-256 of the written Markdown"),
                )
                .example(CommandExample::new(["smith-2019", "--source", "native", "--write"])),
        )
        .command(
            "locate",
            CommandMetadata::new()
                .mutating(false)
                .stability(Stability::Stable)
                .output_field(
                    OutputField::new("source", "string")
                        .description("Source name (default or named)"),
                )
                .output_field(
                    OutputField::new("markdown", "object")
                        .description("Markdown source record: {source, sha256}"),
                )
                .output_field(
                    OutputField::new("pdf", "object")
                        .description("PDF source record: {source, sha256}"),
                )
                .output_field(
                    OutputField::new("claim", "object")
                        .description("Claim evidence entry: {claim, claim_sha256, locators}"),
                )
                .output_field(
                    OutputField::new("pdf_match", "object").description(
                        "PDF match diagnostic: {page, backend, bbox?, geometry_note?, mean_confidence?}",
                    ),
                )
                .example(CommandExample::new([
                    "smith-2019",
                    "--claim",
                    "0",
                    "--exact",
                    "no measurable turbulent mixing was observed",
                    "--page",
                    "3",
                ])),
        )
        .command(
            "check",
            CommandMetadata::new()
                .mutating(false)
                .stability(Stability::Stable)
                .output_field(
                    OutputField::new("valid", "integer").description("Number of valid summaries"),
                )
                .output_field(
                    OutputField::new("skipped", "integer")
                        .description("Summaries without evidence (skipped unless targeted)"),
                )
                .output_field(
                    OutputField::new("invalid", "integer")
                        .description("Summaries with validation errors"),
                )
                .output_field(
                    OutputField::new("summaries", "object[]")
                        .description("Per-summary validation reports: {id, issues}"),
                )
                .example(CommandExample::new(["smith-2019"])),
        )
        .command(
            "audit",
            CommandMetadata::new()
                .mutating(false)
                .stability(Stability::Stable)
                .output_field(
                    OutputField::new("valid", "integer")
                        .description("Summaries with valid evidence"),
                )
                .output_field(
                    OutputField::new("missing", "integer")
                        .description("Summaries without evidence"),
                )
                .output_field(
                    OutputField::new("invalid", "integer")
                        .description("Summaries with invalid evidence"),
                )
                .output_field(
                    OutputField::new("summaries", "object[]")
                        .description("Per-summary audit status: {id, status, issues}"),
                )
                .example(CommandExample::new(["--strict"])),
        )
        .command(
            "propose",
            CommandMetadata::new()
                .mutating(false)
                .stability(Stability::Stable)
                .output_field(
                    OutputField::new("summaries", "object[]")
                        .description("Per-summary proposal reports: {id, claims or issues}"),
                )
                .example(CommandExample::new([
                    "smith-2019",
                    "--all",
                    "--candidates",
                    "5",
                ])),
        );

    for &code in issue_code::ALL {
        let Some(exit_code) = code.exit_code() else {
            continue;
        };
        metadata = metadata.error(
            ErrorMetadata::new(code.as_str())
                .exit_code(exit_code)
                .retryable(false)
                .description(code.description()),
        );
    }
    metadata
}

fn doctor(corpus: &Corpus, tools: &PdfTools, json: bool, quiet: bool) -> Result<()> {
    let ocr = corpus.ocr_config();
    let ocr_dpi = crate::config::validated_ocr_dpi(ocr.dpi)?;

    let corpus_value = corpus.root().display().to_string();
    let config_value = corpus
        .config_file()
        .map_or_else(|| "defaults".to_owned(), |path| path.display().to_string());
    let (mutool_path_ok, mutool_path_value) = match tools.mutool.executable() {
        Ok(path) => (true, path.display().to_string()),
        Err(error) => (false, error.to_string()),
    };
    let (mutool_status, mutool_value) =
        probe_tool_version(tools.mutool.version(), crate::pdf::mutool::validate_version);
    let (tesseract_path_ok, tesseract_path_value) = match tools.tesseract.executable() {
        Ok(path) => (true, path.display().to_string()),
        Err(error) => (false, error.to_string()),
    };
    let (tesseract_status, tesseract_value) = probe_tool_version(
        tools.tesseract.version(),
        crate::pdf::tesseract::validate_version,
    );
    let native_profile_value = crate::pdf::mutool::PROFILE_NAME.to_owned();
    let ocr_profile_value = crate::pdf::tesseract::profile_name(&ocr.lang, ocr_dpi);
    let normalize_value = corpus.normalize_config().options().describe();

    fs::create_dir_all(tools.cache.root()).with_context(|| {
        format!(
            "failed to create runtime cache {}",
            tools.cache.root().display()
        )
    })?;
    let probe = tools
        .cache
        .root()
        .join(format!(".doctor-{}", std::process::id()));
    let (cache_ok, cache_value) =
        match fs::write(&probe, b"receipts doctor").and_then(|()| fs::remove_file(&probe)) {
            Ok(()) => (true, tools.cache.root().display().to_string()),
            Err(error) => (false, error.to_string()),
        };

    let all_ok = mutool_path_ok
        && mutool_status == ToolStatus::Ok
        && tesseract_path_ok
        && tesseract_status == ToolStatus::Ok
        && cache_ok;
    let checks = [
        ("corpus", "ok", &corpus_value),
        ("config", "ok", &config_value),
        (
            "mutool path",
            if mutool_path_ok { "ok" } else { "missing" },
            &mutool_path_value,
        ),
        ("mutool", mutool_status.as_str(), &mutool_value),
        (
            "tesseract path",
            if tesseract_path_ok { "ok" } else { "missing" },
            &tesseract_path_value,
        ),
        ("tesseract", tesseract_status.as_str(), &tesseract_value),
        ("native profile", "ok", &native_profile_value),
        ("OCR profile", "ok", &ocr_profile_value),
        ("normalize", "ok", &normalize_value),
        ("cache", if cache_ok { "ok" } else { "error" }, &cache_value),
    ];

    if json {
        let report = serde_json::json!({
            "corpus": corpus_value,
            "config": config_value,
            "mutool_path": mutool_path_value,
            "mutool_status": mutool_status.as_str(),
            "mutool": mutool_value,
            "tesseract_path": tesseract_path_value,
            "tesseract_status": tesseract_status.as_str(),
            "tesseract": tesseract_value,
            "native_profile": native_profile_value,
            "ocr_profile": ocr_profile_value,
            "normalize": normalize_value,
            "cache": cache_value,
        });
        print_json(&report)?;
    } else if !quiet {
        for (name, status, detail) in &checks {
            print_line(format_args!("{name}: {status} ({detail})"))?;
        }
    }
    if !all_ok {
        bail!("doctor found unavailable requirements");
    }
    Ok(())
}

fn probe_tool_version(
    version: Result<String>,
    validate: fn(&str) -> Result<()>,
) -> (ToolStatus, String) {
    match version {
        Ok(version) => match validate(&version) {
            Ok(()) => (ToolStatus::Ok, version),
            Err(error) => (ToolStatus::Unsupported, error.to_string()),
        },
        Err(error) => (ToolStatus::Missing, error.to_string()),
    }
}

fn locate(corpus: &Corpus, tools: &PdfTools, args: &LocateArgs, json: bool) -> Result<()> {
    if args.column.is_some() && args.line.is_none() {
        bail!("--column requires --line");
    }
    let summary_path = corpus.summary_path(&args.id)?;
    let summary = parse_summary(&corpus.read_contained_text(&summary_path)?, corpus.terms())?;
    if summary.id != args.id {
        bail!(
            "summary ID {:?} does not match filename ID {:?}",
            summary.id,
            args.id
        );
    }
    let claim = summary
        .claims
        .get(args.claim.get())
        .with_context(|| format!("claim {} is out of range", args.claim))?;
    let exact = normalize(&args.exact);
    if exact.is_empty() {
        bail!("--exact is empty after whitespace normalization");
    }

    let markdown_path = resolve_markdown_for(corpus, &args.id, &args.source)?;
    let markdown_source = corpus.read_contained_text(&markdown_path)?;
    let candidates: Vec<_> = parse_units(&markdown_source)
        .into_iter()
        .filter(|unit| args.line.is_none_or(|line| unit.line == line))
        .filter(|unit| args.column.is_none_or(|column| unit.column == column))
        .filter(|unit| exact_count(&unit.text, &exact) == 1)
        .collect();
    let [unit] = candidates.as_slice() else {
        let details = candidates
            .iter()
            .map(|unit| {
                format!(
                    "{} at {}:{}: {:?}",
                    unit.kind, unit.line, unit.column, unit.text
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        bail!(
            "exact text resolves to {} Markdown units; choose unique text or pass coordinates{}",
            candidates.len(),
            if details.is_empty() {
                String::new()
            } else {
                format!("; candidates: {details}")
            }
        );
    };

    let pdf_path = corpus.pdf_path_for(&args.id, &args.source)?;
    if !pdf_path.is_file() {
        bail!("canonical PDF is missing: {}", pdf_path.display());
    }
    let pdf_sha256 = sha256_file(&pdf_path)?;
    let (page, backend, bbox, mean_confidence) = locate_pdf(
        tools,
        &pdf_path,
        &pdf_sha256,
        args.page,
        &exact,
        corpus.ocr_config().enabled,
    )?;
    let result = LocateResult {
        source: args.source.clone(),
        markdown: SourceRecord {
            source: relative(corpus, &markdown_path),
            sha256: sha256_file(&markdown_path)?,
        },
        pdf: SourceRecord {
            source: relative(corpus, &pdf_path),
            sha256: pdf_sha256,
        },
        claim: ClaimEvidence {
            claim: args.claim,
            claim_sha256: crate::hash::sha256_bytes(claim.as_bytes()),
            locators: vec![Locator {
                source: args.source.clone(),
                exact,
                markdown: MarkdownLocator {
                    line: unit.line,
                    column: unit.column,
                    unit: unit.kind,
                    section: unit.section.clone(),
                },
                pdf: PdfLocator { page, backend },
            }],
        },
        pdf_match: Some(PdfMatchDiagnostic {
            page,
            backend,
            bbox,
            geometry_note: bbox
                .is_none()
                .then_some("no geometry available for this backend"),
            mean_confidence,
        }),
    };
    let mut payload = serde_json::to_value(&result).context("failed to encode evidence record")?;
    corpus.terms().localize_locate(&mut payload);
    if json {
        print_json(&payload)
    } else {
        print_locate_yaml(&payload, corpus.terms())
    }
}

fn locate_pdf(
    tools: &PdfTools,
    pdf: &Path,
    pdf_sha256: &str,
    page: Option<Page>,
    exact: &str,
    ocr_enabled: bool,
) -> Result<(Page, PdfBackend, Option<PdfBbox>, Option<f64>)> {
    let native = tools.native_pages(pdf, page)?;
    let native_matches: Vec<_> = native
        .iter()
        .filter_map(|candidate| {
            let count = exact_count(&candidate.text, exact);
            (count > 0).then_some((candidate.page, count))
        })
        .collect();
    match native_matches.as_slice() {
        [(page, 1)] => {
            let bbox = native
                .iter()
                .find(|candidate| candidate.page == *page)
                .and_then(|candidate| matching_bbox(candidate, exact));
            return Ok((*page, PdfBackend::MutoolNative, bbox, None));
        }
        [] => {}
        [(page, count)] => {
            bail!("exact text occurs {count} times on native PDF page {page}");
        }
        matches => {
            bail!(
                "exact text occurs on {} native PDF pages; pass a physical page",
                matches.len()
            );
        }
    }
    let Some(page) = page else {
        bail!("exact text was not found natively; pass --page to permit OCR fallback");
    };
    if !ocr_enabled {
        bail!("exact text was not found natively and OCR is disabled in configuration");
    }
    let ocr = tools.ocr_page(pdf, pdf_sha256, page)?;
    match exact_count(&ocr.text, exact) {
        1 => Ok((
            page,
            PdfBackend::TesseractOcr,
            matching_bbox(&ocr, exact),
            ocr.mean_confidence,
        )),
        0 => bail!("exact text was not found through OCR on physical PDF page {page}"),
        count => bail!("exact text occurs {count} times through OCR on physical PDF page {page}"),
    }
}

fn evaluate_summary(
    corpus: &Corpus,
    tools: &PdfTools,
    id: &str,
    require_review: bool,
    missing_evidence: MissingEvidencePolicy,
) -> SummaryOutcome {
    let summary = match read_summary(corpus, id) {
        Ok(summary) => summary,
        Err(error) => {
            return SummaryOutcome::ParseFailed(issue(
                issue_code::SUMMARY_PARSE_FAILED,
                Severity::Error,
                error.to_string(),
                None,
                None,
            ));
        }
    };
    if summary.id != id {
        return SummaryOutcome::IdMismatch(issue(
            issue_code::ID_MISMATCH,
            Severity::Error,
            format!("summary ID {:?} does not match filename", summary.id),
            None,
            None,
        ));
    }
    if summary.evidence.is_none() && matches!(missing_evidence, MissingEvidencePolicy::Skip) {
        return SummaryOutcome::NoEvidence;
    }
    SummaryOutcome::Validated(validate_document(corpus, &summary, tools, require_review))
}

fn check(
    corpus: &Corpus,
    tools: &PdfTools,
    ids: &[String],
    json: bool,
    quiet: bool,
    require_review: bool,
) -> Result<()> {
    let targeted = !ids.is_empty();
    let ids = if targeted {
        ids.to_vec()
    } else {
        summary_ids(corpus)?
    };
    let mut report = CheckReport {
        valid: 0,
        skipped: 0,
        invalid: 0,
        summaries: Vec::new(),
    };
    for id in ids {
        match evaluate_summary(
            corpus,
            tools,
            &id,
            require_review,
            if targeted {
                MissingEvidencePolicy::Validate
            } else {
                MissingEvidencePolicy::Skip
            },
        ) {
            SummaryOutcome::ParseFailed(issue) | SummaryOutcome::IdMismatch(issue) => {
                report.invalid += 1;
                report.summaries.push(ValidationReport {
                    id,
                    issues: vec![issue],
                });
            }
            SummaryOutcome::NoEvidence => report.skipped += 1,
            SummaryOutcome::Validated(validation) => {
                if validation.is_valid() {
                    report.valid += 1;
                } else {
                    report.invalid += 1;
                }
                report.summaries.push(validation);
            }
        }
    }
    print_check_report(&report, json, quiet)?;
    if report.invalid > 0 {
        bail!(
            "{} summary or summaries have invalid evidence",
            report.invalid
        );
    }
    Ok(())
}

// Four independent flags, not a state machine in disguise; an enum wrapper
// would just relocate the boolean blindness clippy is warning about.
#[allow(clippy::fn_params_excessive_bools)]
fn audit(
    corpus: &Corpus,
    tools: &PdfTools,
    strict: bool,
    require_review: bool,
    ids: &[String],
    json: bool,
    quiet: bool,
) -> Result<()> {
    let mut report = AuditReport {
        valid: 0,
        missing: 0,
        invalid: 0,
        summaries: Vec::new(),
    };
    let ids = if ids.is_empty() {
        summary_ids(corpus)?
    } else {
        ids.to_vec()
    };
    for id in ids {
        match evaluate_summary(
            corpus,
            tools,
            &id,
            require_review,
            MissingEvidencePolicy::Skip,
        ) {
            SummaryOutcome::ParseFailed(issue) | SummaryOutcome::IdMismatch(issue) => {
                report.invalid += 1;
                report.summaries.push(AuditSummary {
                    id,
                    status: AuditStatus::Invalid,
                    issues: vec![issue],
                });
            }
            SummaryOutcome::NoEvidence => {
                report.missing += 1;
                report.summaries.push(AuditSummary {
                    id,
                    status: AuditStatus::Missing,
                    issues: Vec::new(),
                });
            }
            SummaryOutcome::Validated(validation) if validation.is_valid() => {
                report.valid += 1;
                report.summaries.push(AuditSummary {
                    id,
                    status: AuditStatus::Valid,
                    issues: Vec::new(),
                });
            }
            SummaryOutcome::Validated(validation) => {
                report.invalid += 1;
                report.summaries.push(AuditSummary {
                    id,
                    status: AuditStatus::Invalid,
                    issues: validation.issues,
                });
            }
        }
    }
    if json {
        print_json(&report)?;
    } else if !quiet {
        print_line(format_args!(
            "valid: {}\nmissing: {}\ninvalid: {}",
            report.valid, report.missing, report.invalid
        ))?;
    }
    if !json {
        print_issues(
            report
                .summaries
                .iter()
                .map(|summary| (summary.id.as_str(), summary.issues.as_slice())),
        );
    }
    if report.invalid > 0 || (strict && report.missing > 0) {
        bail!("evidence audit failed");
    }
    Ok(())
}

fn propose_cmd(
    corpus: &Corpus,
    tools: &PdfTools,
    args: &ProposeArgs,
    json: bool,
    quiet: bool,
) -> Result<()> {
    let ids = if args.ids.is_empty() {
        summary_ids(corpus)?
    } else {
        args.ids.clone()
    };
    let mut failed = 0;
    let mut reports = Vec::new();
    for id in &ids {
        let summary = match read_summary(corpus, id) {
            Ok(summary) => summary,
            Err(error) => {
                failed += 1;
                let report = error_report(
                    id.clone(),
                    issue_code::SUMMARY_PARSE_FAILED,
                    error.to_string(),
                );
                if !json {
                    print_issues(std::iter::once((
                        report.id.as_str(),
                        report.issues.as_slice(),
                    )));
                }
                reports.push(ProposalSummary::Failed(report));
                continue;
            }
        };
        if summary.id != *id {
            failed += 1;
            let report = error_report(
                id.clone(),
                issue_code::ID_MISMATCH,
                format!("summary ID {:?} does not match filename", summary.id),
            );
            if !json {
                print_issues(std::iter::once((
                    report.id.as_str(),
                    report.issues.as_slice(),
                )));
            }
            reports.push(ProposalSummary::Failed(report));
            continue;
        }
        let report =
            crate::propose::propose_document(corpus, &summary, tools, args.candidates, args.all)?;
        if !json && !quiet {
            print_propose_human(&report, &summary, corpus.terms())?;
        }
        reports.push(ProposalSummary::Proposed(report));
    }
    if json {
        print_json(&serde_json::json!({ "summaries": reports }))?;
    }
    if failed > 0 {
        bail!("{failed} summary or summaries could not be proposed");
    }
    Ok(())
}

fn print_propose_human(
    report: &crate::propose::ProposalReport,
    summary: &crate::evidence::SummaryDocument,
    terms: &Terms,
) -> Result<()> {
    for proposal in &report.claims {
        let claim_text = summary
            .claims
            .get(proposal.claim.get())
            .map_or("<unknown>", String::as_str);
        print_line(format_args!(
            "# {} {}  {:?}",
            terms.claim,
            proposal.claim,
            truncate(claim_text, CLAIM_PREVIEW_CHARS)
        ))?;
        if !proposal.required_tokens.is_empty() {
            print_line(format_args!(
                "#   required tokens: {}",
                proposal.required_tokens.join(", ")
            ))?;
        }
        if !proposal.uncovered_tokens.is_empty() {
            print_line(format_args!(
                "#   uncovered tokens: {}",
                proposal.uncovered_tokens.join(", ")
            ))?;
        }

        for (index, candidate) in proposal.candidates.iter().enumerate() {
            let section = if candidate.markdown.section.is_empty() {
                String::new()
            } else {
                format!(" [{}]", candidate.markdown.section.join(" > "))
            };
            let pdf = format!(
                "  p.{} {}",
                candidate.pdf.page,
                candidate.pdf.backend.as_str()
            );
            print_line(format_args!(
                "#   [{}] {}/{}  {}  md:{}:{} {}{section}{pdf}",
                candidate_label(index),
                candidate.coverage.matched,
                candidate.coverage.required,
                candidate.source,
                candidate.markdown.line,
                candidate.markdown.column,
                candidate.markdown.unit.as_str(),
            ))?;
            print_line(format_args!("#       {:?}", candidate.exact))?;
        }

        if let Some(first) = proposal.candidates.first() {
            // `--claim` and `--source` are fixed flag names; only prose follows
            // the configured vocabulary, so the pasted command always runs.
            let source = if first.source == DEFAULT_SOURCE {
                String::new()
            } else {
                format!(" --source {:?}", first.source)
            };
            print_line(format_args!("#"))?;
            print_line(format_args!("#   accept [{}]:", candidate_label(0)))?;
            print_line(format_args!(
                "#     receipts locate {} --claim {} \\",
                report.id, proposal.claim
            ))?;
            print_line(format_args!(
                "#       --exact {:?} --page {}{source}",
                first.exact, first.pdf.page
            ))?;
        }
        print_line(format_args!(""))?;
    }
    Ok(())
}

/// Longest claim preview kept intact before an ellipsis.
const CLAIM_PREVIEW_CHARS: usize = 72;

/// Candidate labels run `a`..`z`, then fall back to the ordinal.
fn candidate_label(index: usize) -> String {
    match u8::try_from(index) {
        Ok(offset) if offset < 26 => char::from(b'a' + offset).to_string(),
        _ => index.to_string(),
    }
}

/// Truncate on a character boundary, so non-ASCII claims never panic.
fn truncate(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{head}...")
    } else {
        head
    }
}

fn read_summary(corpus: &Corpus, id: &str) -> Result<crate::evidence::SummaryDocument> {
    let path = corpus.summary_path(id)?;
    let source = corpus.read_contained_text(&path)?;
    parse_summary(&source, corpus.terms())
        .with_context(|| format!("failed to parse {}", path.display()))
}

fn summary_ids(corpus: &Corpus) -> Result<Vec<String>> {
    let template = corpus.summary_template();
    let Some((prefix, suffix)) = template.split_once("{id}") else {
        bail!("summary template missing {{id}} placeholder");
    };

    let scan_dir = corpus.summaries_dir();
    let max_depth = suffix.bytes().filter(|byte| *byte == b'/').count();
    let mut ids = Vec::new();
    walk_summaries(
        &scan_dir,
        prefix,
        suffix,
        corpus.root(),
        0,
        max_depth,
        &mut ids,
    )?;
    ids.sort();
    ids.dedup();
    Ok(ids)
}

fn walk_summaries(
    dir: &Path,
    prefix: &str,
    suffix: &str,
    root: &Path,
    depth: usize,
    max_depth: usize,
    ids: &mut Vec<String>,
) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(
                anyhow::Error::new(error).context(format!("failed to read {}", dir.display()))
            );
        }
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", path.display()))?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if depth >= max_depth {
                bail!(
                    "summary discovery exceeded template depth {max_depth}: {}",
                    path.display()
                );
            }
            walk_summaries(&path, prefix, suffix, root, depth + 1, max_depth, ids)?;
        } else {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if let Some(id) = relative
                .strip_prefix(prefix)
                .and_then(|r| r.strip_suffix(suffix))
                && !id.is_empty()
                && !id.contains('/')
            {
                ids.push(id.to_owned());
            }
        }
    }
    Ok(())
}

fn resolve_markdown_for(corpus: &Corpus, id: &str, source: &str) -> Result<PathBuf> {
    let candidates = corpus.markdown_candidates_for(id, source)?;
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .with_context(|| format!("no canonical Markdown source found for {id} (source: {source})"))
}

fn relative(corpus: &Corpus, path: &Path) -> String {
    path.strip_prefix(corpus.root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn print_check_report(report: &CheckReport, json: bool, quiet: bool) -> Result<()> {
    if json {
        return print_json(report);
    }
    if !quiet {
        print_line(format_args!(
            "valid: {}\nskipped: {}\ninvalid: {}",
            report.valid, report.skipped, report.invalid
        ))?;
    }
    print_issues(
        report
            .summaries
            .iter()
            .map(|summary| (summary.id.as_str(), summary.issues.as_slice())),
    );
    Ok(())
}

fn print_issues<'a>(summaries: impl IntoIterator<Item = (&'a str, &'a [EvidenceIssue])>) {
    for (id, issues) in summaries {
        for issue in issues {
            let prefix = match issue.severity {
                Severity::Warning => "warning: ",
                Severity::Error => "",
            };
            eprintln!("{id}: {prefix}{}: {}", issue.code, issue.message);
        }
    }
}

fn error_report(id: String, code: IssueCode, message: impl Into<String>) -> ValidationReport {
    ValidationReport {
        id,
        issues: vec![issue(code, Severity::Error, message, None, None)],
    }
}

fn print_locate_yaml(payload: &serde_json::Value, terms: &Terms) -> Result<()> {
    let field = |name: &str| {
        payload
            .get(name)
            .unwrap_or(&serde_json::Value::Null)
            .clone()
    };
    let markdown = serde_json::to_string(&field("markdown"))?;
    let pdf = serde_json::to_string(&field("pdf"))?;
    let entry = serde_json::to_string_pretty(&field(&terms.claim))?;
    if let Some(pdf_match) = payload.get("pdf_match") {
        let diagnostic = serde_json::to_string(pdf_match)?;
        print_line(format_args!("# PDF match diagnostic: {diagnostic}"))?;
    }
    print_line(format_args!(
        "markdown: {markdown}\npdf: {pdf}\n{}: {entry}",
        terms.claim
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::cache::CacheReadPolicy;

    #[test]
    fn corpus_local_cache_requires_explicit_cli_trust() {
        let default = Cli::try_parse_from(["receipts", "doctor"]).unwrap();
        let opted_in = Cli::try_parse_from(["receipts", "--trust-cache", "doctor"]).unwrap();

        assert!(!default.trust_cache);
        assert!(opted_in.trust_cache);
        assert_eq!(cache_read_policy(true, false), CacheReadPolicy::WriteOnly);
        assert_eq!(cache_read_policy(false, false), CacheReadPolicy::Trusted);
        assert_eq!(cache_read_policy(true, true), CacheReadPolicy::Trusted);
    }

    #[test]
    fn missing_pdf_geometry_is_explicit_in_diagnostics() {
        let diagnostic = PdfMatchDiagnostic {
            page: Page::new(1).unwrap(),
            backend: PdfBackend::MutoolNative,
            bbox: None,
            geometry_note: Some("no geometry available for this backend"),
            mean_confidence: None,
        };

        let value = serde_json::to_value(diagnostic).unwrap();

        assert_eq!(
            value["geometry_note"],
            "no geometry available for this backend"
        );
        assert!(value.get("bbox").is_none());
    }
}
