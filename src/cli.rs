use std::{
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
    corpus::Corpus,
    evidence::{
        ClaimEvidence, DEFAULT_SOURCE, Locator, MarkdownLocator, PdfBackend, PdfLocator, Severity,
        SourceRecord, parse_summary,
    },
    hash::sha256_file,
    markdown::{exact_count, parse_units},
    normalize::normalize,
    output::print_json,
    pdf::{PdfBbox, PdfTextProvider, PdfTools, matching_bbox},
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
PDF page, after Unicode whitespace-run normalization and nothing else.
Ambiguity is an error, not an occurrence to choose from.

Corpus layout is declared in receipts.yaml, discovered by walking up from the
working directory."
)]
pub struct Cli {
    #[command(flatten)]
    common: librebar::cli::CommonArgs,
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
}

#[derive(Debug, Args)]
struct LocateArgs {
    id: String,
    #[arg(long)]
    claim: usize,
    #[arg(long)]
    exact: String,
    #[arg(long, default_value = DEFAULT_SOURCE)]
    source: String,
    #[arg(long)]
    page: Option<usize>,
    #[arg(long)]
    line: Option<usize>,
    #[arg(long)]
    column: Option<usize>,
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
    page: usize,
    backend: PdfBackend,
    #[serde(skip_serializing_if = "Option::is_none")]
    bbox: Option<PdfBbox>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mean_confidence: Option<f64>,
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
    let format = cli.common.output_format();
    let json = format == ResolvedOutputFormat::Json;
    let quiet = cli.common.quiet;
    match cli.command {
        Command::Doctor => doctor(&corpus, json, quiet),
        Command::Locate(args) => locate(&corpus, &args, json),
        Command::Check(args) => check(&corpus, &args.ids, json, quiet, args.require_review),
        Command::Audit(args) => audit(
            &corpus,
            args.strict,
            args.require_review,
            &args.ids,
            json,
            quiet,
        ),
        Command::Propose(args) => propose_cmd(&corpus, &args, json, quiet),
    }
}

fn schema_metadata() -> SchemaMetadata {
    SchemaMetadata::new()
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
                .output_field(OutputField::new("cache", "string").description("Cache root path")),
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
                        "PDF match diagnostic: {page, backend, bbox?, mean_confidence?}",
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
                .output_field(OutputField::new("id", "string").description("Summary document ID"))
                .output_field(OutputField::new("claims", "object[]").description(
                    "Per-claim proposals: {claim, required_tokens, uncovered_tokens, candidates}",
                ))
                .example(CommandExample::new([
                    "smith-2019",
                    "--all",
                    "--candidates",
                    "5",
                ])),
        )
        .error(
            ErrorMetadata::new("missing_evidence")
                .exit_code(1)
                .retryable(false)
                .description("Summary has no evidence section"),
        )
        .error(
            ErrorMetadata::new("stale_hash")
                .exit_code(1)
                .retryable(false)
                .description("Claim or source SHA-256 does not match current content"),
        )
        .error(
            ErrorMetadata::new("markdown_missing")
                .exit_code(1)
                .retryable(false)
                .description("Exact text not found in the recorded Markdown unit"),
        )
        .error(
            ErrorMetadata::new("markdown_ambiguous")
                .exit_code(1)
                .retryable(false)
                .description("Exact text occurs more than once in the Markdown unit"),
        )
        .error(
            ErrorMetadata::new("pdf_missing")
                .exit_code(1)
                .retryable(false)
                .description("Exact text not found on the PDF page"),
        )
        .error(
            ErrorMetadata::new("pdf_ambiguous")
                .exit_code(1)
                .retryable(false)
                .description("Exact text occurs more than once on the PDF page"),
        )
        .error(
            ErrorMetadata::new("unknown_source")
                .exit_code(1)
                .retryable(false)
                .description("Locator references an undeclared source"),
        )
        .error(
            ErrorMetadata::new("unused_source")
                .exit_code(1)
                .retryable(false)
                .description("Declared source is not cited by any locator"),
        )
        .error(
            ErrorMetadata::new("unknown_source_template")
                .exit_code(1)
                .retryable(false)
                .description("No configured templates for a declared source name"),
        )
        .error(
            ErrorMetadata::new("uncovered_token")
                .exit_code(1)
                .retryable(false)
                .description("A required claim token appears in no locator"),
        )
        .error(
            ErrorMetadata::new("stale_section")
                .exit_code(1)
                .retryable(false)
                .description("Recorded section path disagrees with the source"),
        )
        .error(
            ErrorMetadata::new("ocr_disabled")
                .exit_code(1)
                .retryable(false)
                .description("Locator uses OCR but OCR is disabled in configuration"),
        )
        .error(
            ErrorMetadata::new("stale_review_claim")
                .exit_code(1)
                .retryable(false)
                .description("Review claim_sha256 does not match current claim text"),
        )
        .error(
            ErrorMetadata::new("stale_review_evidence")
                .exit_code(1)
                .retryable(false)
                .description("Review evidence_sha256 does not match current locator set"),
        )
        .error(
            ErrorMetadata::new("unknown_review_claim")
                .exit_code(1)
                .retryable(false)
                .description("Review entry references a nonexistent claim"),
        )
        .error(
            ErrorMetadata::new("duplicate_review_claim")
                .exit_code(1)
                .retryable(false)
                .description("Two review entries for one claim"),
        )
        .error(
            ErrorMetadata::new("missing_review")
                .exit_code(1)
                .retryable(false)
                .description("Claim has no review entry (under --require-review)"),
        )
        .error(
            ErrorMetadata::new("unsupported_verdict")
                .exit_code(1)
                .retryable(false)
                .description("Verdict is not 'supported' (under --require-review)"),
        )
}

fn doctor(corpus: &Corpus, json: bool, quiet: bool) -> Result<()> {
    let ocr = corpus.ocr_config();
    let tools = PdfTools::new(corpus.cache_root().to_path_buf(), ocr);
    let ocr_dpi = u16::try_from(ocr.dpi).unwrap_or(u16::MAX);

    let corpus_value = corpus.root().display().to_string();
    let config_value = corpus
        .config_file()
        .map_or_else(|| "defaults".to_owned(), |path| path.display().to_string());
    let (mutool_ok, mutool_value) = match tools.mutool.version() {
        Ok(version) => (true, version),
        Err(error) => (false, error.to_string()),
    };
    let (tesseract_ok, tesseract_value) = match tools.tesseract.version() {
        Ok(version) => (true, version),
        Err(error) => (false, error.to_string()),
    };
    let native_profile_value = crate::pdf::mutool::PROFILE_NAME.to_owned();
    let ocr_profile_value = crate::pdf::tesseract::profile_name(&ocr.lang, ocr_dpi);

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

    let all_ok = mutool_ok && tesseract_ok && cache_ok;
    let checks = [
        ("corpus", true, &corpus_value),
        ("config", true, &config_value),
        ("mutool", mutool_ok, &mutool_value),
        ("tesseract", tesseract_ok, &tesseract_value),
        ("native profile", true, &native_profile_value),
        ("OCR profile", true, &ocr_profile_value),
        ("cache", cache_ok, &cache_value),
    ];

    if json {
        let report = serde_json::json!({
            "corpus": corpus_value,
            "config": config_value,
            "mutool": mutool_value,
            "tesseract": tesseract_value,
            "native_profile": native_profile_value,
            "ocr_profile": ocr_profile_value,
            "cache": cache_value,
        });
        print_json(&report)?;
    } else if !quiet {
        for (name, valid, detail) in &checks {
            println!("{name}: {} ({detail})", if *valid { "ok" } else { "error" });
        }
    }
    if !all_ok {
        bail!("doctor found unavailable requirements");
    }
    Ok(())
}

fn locate(corpus: &Corpus, args: &LocateArgs, json: bool) -> Result<()> {
    if args.page == Some(0) || args.line == Some(0) || args.column == Some(0) {
        bail!("page, line, and column are one-based");
    }
    if args.column.is_some() && args.line.is_none() {
        bail!("--column requires --line");
    }
    let summary_path = corpus.summary_path(&args.id)?;
    let summary = parse_summary(
        &fs::read_to_string(&summary_path)
            .with_context(|| format!("failed to read {}", summary_path.display()))?,
        corpus.terms(),
    )?;
    if summary.id != args.id {
        bail!(
            "summary ID {:?} does not match filename ID {:?}",
            summary.id,
            args.id
        );
    }
    let claim = summary
        .claims
        .get(args.claim)
        .with_context(|| format!("claim {} is out of range", args.claim))?;
    let exact = normalize(&args.exact);
    if exact.is_empty() {
        bail!("--exact is empty after whitespace normalization");
    }

    let markdown_path = resolve_markdown_for(corpus, &args.id, &args.source)?;
    let markdown_source = fs::read_to_string(&markdown_path)
        .with_context(|| format!("failed to read {}", markdown_path.display()))?;
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
                    "{:?} at {}:{}: {:?}",
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
    let tools = PdfTools::new(corpus.cache_root().to_path_buf(), corpus.ocr_config());
    let (page, backend, bbox, mean_confidence) = locate_pdf(
        &tools,
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
    page: Option<usize>,
    exact: &str,
    ocr_enabled: bool,
) -> Result<(usize, PdfBackend, Option<PdfBbox>, Option<f64>)> {
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

fn check(
    corpus: &Corpus,
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
    let tools = PdfTools::new(corpus.cache_root().to_path_buf(), corpus.ocr_config());
    let mut report = CheckReport {
        valid: 0,
        skipped: 0,
        invalid: 0,
        summaries: Vec::new(),
    };
    for id in ids {
        let summary = match read_summary(corpus, &id) {
            Ok(summary) => summary,
            Err(error) => {
                report.invalid += 1;
                report
                    .summaries
                    .push(error_report(id, "summary_parse_failed", error.to_string()));
                continue;
            }
        };
        if summary.id != id {
            report.invalid += 1;
            report.summaries.push(error_report(
                id,
                "id_mismatch",
                format!("summary ID {:?} does not match filename", summary.id),
            ));
            continue;
        }
        if summary.evidence.is_none() && !targeted {
            report.skipped += 1;
            continue;
        }
        let validation = validate_document(corpus, &summary, &tools, require_review);
        if validation.is_valid() {
            report.valid += 1;
        } else {
            report.invalid += 1;
        }
        report.summaries.push(validation);
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
    strict: bool,
    require_review: bool,
    ids: &[String],
    json: bool,
    quiet: bool,
) -> Result<()> {
    let tools = PdfTools::new(corpus.cache_root().to_path_buf(), corpus.ocr_config());
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
        let summary = match read_summary(corpus, &id) {
            Ok(summary) => summary,
            Err(error) => {
                report.invalid += 1;
                report.summaries.push(AuditSummary {
                    id,
                    status: AuditStatus::Invalid,
                    issues: vec![crate::evidence::EvidenceIssue {
                        code: "summary_parse_failed".to_owned(),
                        severity: Severity::Error,
                        message: error.to_string(),
                        claim: None,
                        locator: None,
                    }],
                });
                continue;
            }
        };
        if summary.id != id {
            report.invalid += 1;
            report.summaries.push(AuditSummary {
                id,
                status: AuditStatus::Invalid,
                issues: vec![crate::evidence::EvidenceIssue {
                    code: "id_mismatch".to_owned(),
                    severity: Severity::Error,
                    message: format!("summary ID {:?} does not match filename", summary.id),
                    claim: None,
                    locator: None,
                }],
            });
            continue;
        }
        if summary.evidence.is_none() {
            report.missing += 1;
            report.summaries.push(AuditSummary {
                id,
                status: AuditStatus::Missing,
                issues: Vec::new(),
            });
            continue;
        }
        let validation = validate_document(corpus, &summary, &tools, require_review);
        if validation.is_valid() {
            report.valid += 1;
            report.summaries.push(AuditSummary {
                id,
                status: AuditStatus::Valid,
                issues: Vec::new(),
            });
        } else {
            report.invalid += 1;
            report.summaries.push(AuditSummary {
                id,
                status: AuditStatus::Invalid,
                issues: validation.issues,
            });
        }
    }
    if json {
        print_json(&report)?;
    } else if !quiet {
        println!(
            "valid: {}\nmissing: {}\ninvalid: {}",
            report.valid, report.missing, report.invalid
        );
    }
    if !json {
        print_audit_issues(&report.summaries);
    }
    if report.invalid > 0 || (strict && report.missing > 0) {
        bail!("evidence audit failed");
    }
    Ok(())
}

fn propose_cmd(corpus: &Corpus, args: &ProposeArgs, json: bool, quiet: bool) -> Result<()> {
    let ids = if args.ids.is_empty() {
        summary_ids(corpus)?
    } else {
        args.ids.clone()
    };
    let tools = PdfTools::new(corpus.cache_root().to_path_buf(), corpus.ocr_config());

    let mut reports = Vec::new();
    for id in &ids {
        let summary = read_summary(corpus, id)?;
        if summary.id != *id {
            bail!(
                "summary ID {:?} does not match filename ID {:?}",
                summary.id,
                id
            );
        }
        let report =
            crate::propose::propose_document(corpus, &summary, &tools, args.candidates, args.all)?;
        if !json && !quiet {
            print_propose_human(&report, &summary, corpus.terms());
        }
        reports.push(report);
    }
    if json {
        if reports.len() == 1 {
            print_json(&reports.into_iter().next().unwrap())?;
        } else {
            print_json(&serde_json::json!({ "summaries": reports }))?;
        }
    }
    Ok(())
}

fn print_propose_human(
    report: &crate::propose::ProposalReport,
    summary: &crate::evidence::SummaryDocument,
    terms: &Terms,
) {
    for proposal in &report.claims {
        let claim_text = summary
            .claims
            .get(proposal.claim)
            .map_or("<unknown>", String::as_str);
        println!(
            "# {} {}  {:?}",
            terms.claim,
            proposal.claim,
            truncate(claim_text, CLAIM_PREVIEW_CHARS)
        );
        if !proposal.required_tokens.is_empty() {
            println!(
                "#   required tokens: {}",
                proposal.required_tokens.join(", ")
            );
        }
        if !proposal.uncovered_tokens.is_empty() {
            println!(
                "#   uncovered tokens: {}",
                proposal.uncovered_tokens.join(", ")
            );
        }

        for (index, candidate) in proposal.candidates.iter().enumerate() {
            let section = if candidate.markdown.section.is_empty() {
                String::new()
            } else {
                format!(" [{}]", candidate.markdown.section.join(" > "))
            };
            let pdf = candidate.pdf.as_ref().map_or_else(
                || "  (no native PDF match)".to_owned(),
                |pdf| format!("  p.{} {}", pdf.page, pdf.backend.as_str()),
            );
            println!(
                "#   [{}] {}/{}  {}  md:{}:{} {}{section}{pdf}",
                candidate_label(index),
                candidate.coverage.matched,
                candidate.coverage.required,
                candidate.source,
                candidate.markdown.line,
                candidate.markdown.column,
                candidate.markdown.unit.as_str(),
            );
            println!("#       {:?}", candidate.exact);
        }

        if let Some(first) = proposal.candidates.first()
            && let Some(pdf) = first.pdf.as_ref()
        {
            // `--claim` and `--source` are fixed flag names; only prose follows
            // the configured vocabulary, so the pasted command always runs.
            let source = if first.source == DEFAULT_SOURCE {
                String::new()
            } else {
                format!(" --source {:?}", first.source)
            };
            println!("#");
            println!("#   accept [{}]:", candidate_label(0));
            println!(
                "#     receipts locate {} --claim {} \\",
                report.id, proposal.claim
            );
            println!(
                "#       --exact {:?} --page {}{source}",
                first.exact, pdf.page
            );
        }
        println!();
    }
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
    let source =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    parse_summary(&source, corpus.terms())
        .with_context(|| format!("failed to parse {}", path.display()))
}

fn summary_ids(corpus: &Corpus) -> Result<Vec<String>> {
    let directory = corpus.summaries_dir();
    let mut ids = Vec::new();
    for entry in fs::read_dir(&directory)
        .with_context(|| format!("failed to read {}", directory.display()))?
    {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) == Some("yaml")
            && let Some(id) = path.file_stem().and_then(|value| value.to_str())
        {
            ids.push(id.to_owned());
        }
    }
    ids.sort();
    Ok(ids)
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
        println!(
            "valid: {}\nskipped: {}\ninvalid: {}",
            report.valid, report.skipped, report.invalid
        );
    }
    print_issues(&report.summaries);
    Ok(())
}

fn print_issues(reports: &[ValidationReport]) {
    for report in reports {
        for issue in &report.issues {
            let prefix = match issue.severity {
                Severity::Warning => "warning: ",
                Severity::Error => "",
            };
            eprintln!("{}: {prefix}{}: {}", report.id, issue.code, issue.message);
        }
    }
}

fn print_audit_issues(summaries: &[AuditSummary]) {
    for summary in summaries {
        for issue in &summary.issues {
            let prefix = match issue.severity {
                Severity::Warning => "warning: ",
                Severity::Error => "",
            };
            eprintln!("{}: {prefix}{}: {}", summary.id, issue.code, issue.message);
        }
    }
}

fn error_report(
    id: String,
    code: impl Into<String>,
    message: impl Into<String>,
) -> ValidationReport {
    ValidationReport {
        id,
        issues: vec![crate::evidence::EvidenceIssue {
            code: code.into(),
            severity: Severity::Error,
            message: message.into(),
            claim: None,
            locator: None,
        }],
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
        println!("# PDF match diagnostic: {diagnostic}");
    }
    println!("markdown: {markdown}\npdf: {pdf}\n{}: {entry}", terms.claim);
    Ok(())
}
