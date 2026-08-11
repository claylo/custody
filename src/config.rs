//! Configuration discovery and corpus-root resolution.
//!
//! Layout is declared in `receipts.yaml`, discovered by walking up from the
//! working directory. The directory containing that file is the corpus root.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use librebar::camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Configuration merged from defaults and repository-scoped project or explicit files.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub corpus: CorpusLayout,
    pub cache: CacheConfig,
    pub pdf: PdfConfig,
    pub terms: crate::terms::Terms,
    pub coverage: CoverageConfig,
    pub sections: SectionsConfig,
}

/// Path templates resolved against the corpus root. `{id}` is substituted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CorpusLayout {
    /// Template for a summary document.
    pub summaries: String,
    /// Markdown/PDF template pairs, keyed by source name.
    pub sources: BTreeMap<String, SourceTemplates>,
}

impl Default for CorpusLayout {
    fn default() -> Self {
        let mut sources = BTreeMap::new();
        sources.insert(
            crate::evidence::DEFAULT_SOURCE.to_owned(),
            SourceTemplates::default(),
        );
        Self {
            summaries: "summaries/{id}.yaml".to_owned(),
            sources,
        }
    }
}

/// Markdown and PDF templates for one named source.
///
/// Both fields are required: a source that silently inherited the default
/// PDF template would resolve two named sources to the same file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceTemplates {
    /// Candidate templates for converted Markdown, tried in order.
    pub markdown: Vec<String>,
    /// Template for the canonical PDF.
    pub pdf: String,
}

impl Default for SourceTemplates {
    fn default() -> Self {
        Self {
            markdown: vec!["md/{id}.md".to_owned(), "md/{id}/{id}.md".to_owned()],
            pdf: "pdfs/{id}.pdf".to_owned(),
        }
    }
}

/// Extraction cache placement.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CacheConfig {
    /// Cache root. `None` uses the platform cache directory. Configured paths
    /// must be relative to and resolve within the corpus root.
    pub root: Option<String>,
}

/// PDF extraction settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PdfConfig {
    pub tools: PdfToolConfig,
    pub ocr: OcrConfig,
}

/// Executables used for PDF extraction. Unset paths are resolved from `PATH`
/// once when the PDF tools are constructed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PdfToolConfig {
    /// Absolute path to `mutool`.
    pub mutool: Option<PathBuf>,
    /// Absolute path to `tesseract`.
    pub tesseract: Option<PathBuf>,
}

/// OCR fallback settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OcrConfig {
    /// Whether OCR fallback is permitted at all.
    pub enabled: bool,
    /// Render resolution for OCR, in dots per inch. Valid values are 1–1200.
    pub dpi: u32,
    /// Tesseract language code.
    pub lang: String,
    /// Tesseract page segmentation mode (`--psm`), 0-13.
    pub page_segmentation_mode: u8,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dpi: 300,
            lang: "eng".to_owned(),
            page_segmentation_mode: 3,
        }
    }
}

/// How a claim token that no locator covers is reported.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenSeverity {
    #[default]
    Error,
    Warn,
    Off,
}

// Hand-written so `tokens: off` works unquoted: the YAML layer resolves the
// bare scalar `off` to the boolean `false` before serde ever sees a string.
impl<'de> Deserialize<'de> for TokenSeverity {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;

        match Value::deserialize(deserializer)? {
            Value::String(name) => match name.as_str() {
                "error" => Ok(Self::Error),
                "warn" => Ok(Self::Warn),
                "off" => Ok(Self::Off),
                other => Err(D::Error::unknown_variant(other, &["error", "warn", "off"])),
            },
            Value::Bool(false) => Ok(Self::Off),
            other => Err(D::Error::invalid_type(
                unexpected(&other),
                &"one of \"error\", \"warn\", or \"off\"",
            )),
        }
    }
}

fn unexpected(value: &Value) -> serde::de::Unexpected<'_> {
    use serde::de::Unexpected;
    match value {
        Value::Bool(value) => Unexpected::Bool(*value),
        Value::Null => Unexpected::Unit,
        Value::Number(_) => Unexpected::Other("number"),
        Value::Array(_) => Unexpected::Seq,
        Value::Object(_) => Unexpected::Map,
        Value::String(value) => Unexpected::Str(value),
    }
}

/// Which claim-token gaps are reported, and how loudly.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CoverageConfig {
    pub tokens: TokenSeverity,
}

/// Sections whose evidence is treated as weak.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SectionsConfig {
    /// Heading substrings that mark a section as weak evidence.
    pub weak: Vec<String>,
}

/// A resolved configuration together with where it came from.
#[derive(Debug, Clone)]
pub struct Discovered {
    pub config: Config,
    pub root: PathBuf,
    /// Project config file backing this corpus, if one was found.
    pub config_file: Option<PathBuf>,
}

/// Load configuration and resolve the corpus root.
///
/// Root resolution, in order: the directory holding the discovered config
/// file; else the nearest `.git` boundary; else `start`.
pub fn load(start: &Path, explicit: Option<&Path>) -> Result<Discovered> {
    let search = to_utf8(start)?;
    let mut loader = librebar::config::ConfigLoader::new("receipts")
        .with_project_search(&search)
        .with_user_config(false)
        .without_environment();
    if let Some(path) = explicit {
        loader = loader.with_file(to_utf8(path)?);
    }
    // Loaded as a `Value` so desugaring sees only what the files declared:
    // seeding the merge with `Config::default()` would make every corpus look
    // like it had declared `sources`.
    let (mut raw, sources): (Value, _) = loader
        .load()
        .map_err(|error| anyhow!("failed to load receipts configuration: {error}"))?;
    if raw.is_null() {
        raw = Value::Object(Map::new());
    }
    desugar_corpus_layout(&mut raw)?;
    let config: Config =
        serde_json::from_value(raw).context("failed to deserialize receipts configuration")?;
    validate(&config)?;
    let root = resolve_root(sources.project_file.as_deref(), start)?;
    let config_file = sources
        .project_file
        .as_deref()
        .map(|path| PathBuf::from(path.as_str()));
    Ok(Discovered {
        config,
        root,
        config_file,
    })
}

fn to_utf8(path: &Path) -> Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(path.to_path_buf())
        .map_err(|value| anyhow!("path is not valid UTF-8: {}", value.display()))
}

/// Rewrite the single-source `corpus.markdown`/`corpus.pdf` shorthand into the
/// general `corpus.sources` map.
fn desugar_corpus_layout(value: &mut Value) -> Result<()> {
    let Some(corpus) = value.get_mut("corpus").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    let has_bare = corpus.contains_key("markdown") || corpus.contains_key("pdf");
    if !has_bare {
        return Ok(());
    }
    if corpus.contains_key("sources") {
        bail!("corpus must declare either markdown/pdf or sources, not both");
    }
    let defaults = SourceTemplates::default();
    let markdown = corpus.remove("markdown").unwrap_or_else(|| {
        defaults
            .markdown
            .into_iter()
            .map(Value::String)
            .collect::<Vec<_>>()
            .into()
    });
    let pdf = corpus.remove("pdf").unwrap_or(Value::String(defaults.pdf));
    let mut templates = Map::new();
    templates.insert("markdown".to_owned(), markdown);
    templates.insert("pdf".to_owned(), pdf);
    let mut sources = Map::new();
    sources.insert(
        crate::evidence::DEFAULT_SOURCE.to_owned(),
        Value::Object(templates),
    );
    corpus.insert("sources".to_owned(), Value::Object(sources));
    Ok(())
}

/// Reject templates that could escape the corpus root.
fn validate(config: &Config) -> Result<()> {
    if let Some(root) = config.cache.root.as_deref() {
        validate_cache_root(root)?;
    }
    validate_template(&config.corpus.summaries)?;
    if config.corpus.sources.is_empty() {
        bail!("corpus.sources must declare at least one source");
    }
    for (name, templates) in &config.corpus.sources {
        if !crate::evidence::validate_source_name(name) {
            bail!("corpus source name {name:?} is not a valid path component");
        }
        if templates.markdown.is_empty() {
            bail!("corpus source {name:?} must list at least one markdown template");
        }
        validate_template(&templates.pdf)?;
        for template in &templates.markdown {
            validate_template(template)?;
        }
    }
    validate_tool_path("mutool", config.pdf.tools.mutool.as_deref())?;
    validate_tool_path("tesseract", config.pdf.tools.tesseract.as_deref())?;
    validated_ocr_dpi(config.pdf.ocr.dpi)?;
    if config.pdf.ocr.lang.is_empty() {
        bail!("pdf.ocr.lang must not be empty");
    }
    if config.pdf.ocr.page_segmentation_mode > 13 {
        bail!("pdf.ocr.page_segmentation_mode must be 0–13");
    }
    config.terms.validate()?;
    Ok(())
}

fn validate_tool_path(name: &str, path: Option<&Path>) -> Result<()> {
    if path.is_some_and(|path| !path.is_absolute()) {
        bail!("pdf.tools.{name} must be an absolute path");
    }
    Ok(())
}

pub(crate) fn validated_ocr_dpi(dpi: u32) -> Result<u16> {
    if !(1..=1200).contains(&dpi) {
        bail!("pdf.ocr.dpi must be 1–1200");
    }
    u16::try_from(dpi).context("pdf.ocr.dpi cannot be represented by the OCR backend")
}

pub(crate) fn validate_cache_root(root: &str) -> Result<()> {
    let path = Path::new(root);
    if path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::Prefix(_)))
    {
        bail!("cache.root must be relative to the corpus root");
    }
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        bail!("cache.root must not contain ..");
    }
    Ok(())
}

fn validate_template(template: &str) -> Result<()> {
    if !template.contains("{id}") {
        bail!("corpus template {template:?} must contain {{id}}");
    }
    if Path::new(template).is_absolute() {
        bail!("corpus template {template:?} must be relative to the corpus root");
    }
    if template.split('/').any(|segment| segment == "..") {
        bail!("corpus template {template:?} must not contain ..");
    }
    Ok(())
}

fn resolve_root(project_file: Option<&Utf8Path>, start: &Path) -> Result<PathBuf> {
    if let Some(file) = project_file {
        let directory = file
            .parent()
            .with_context(|| format!("config file {file} has no parent directory"))?;
        // `.config/receipts.yaml` sits one level below the corpus root.
        let root = if directory.file_name() == Some(".config") {
            directory
                .parent()
                .with_context(|| format!("config file {file} has no corpus root"))?
        } else {
            directory
        };
        // Canonicalize: source containment is checked by comparing a
        // canonicalized source path against this root, and on macOS a
        // non-canonical root (/var vs /private/var) fails every comparison.
        let root = PathBuf::from(root.as_str());
        return root
            .canonicalize()
            .with_context(|| format!("failed to resolve corpus root {}", root.display()));
    }

    let mut current = start
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", start.display()))?;
    if current.is_file() {
        current.pop();
    }
    let fallback = current.clone();
    loop {
        if current.join(".git").exists() {
            return Ok(current);
        }
        if !current.pop() {
            return Ok(fallback);
        }
    }
}

/// Platform cache directory for extraction artifacts.
///
/// `~/Library/Caches/receipts/pdf-text` on macOS.
pub fn platform_cache_root() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "receipts")
        .context("could not determine the platform cache directory")?;
    Ok(dirs.cache_dir().join("pdf-text"))
}
