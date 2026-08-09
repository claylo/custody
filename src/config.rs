//! Configuration discovery and corpus-root resolution.
//!
//! Layout is declared in `receipts.yaml`, discovered by walking up from the
//! working directory. The directory containing that file is the corpus root.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use librebar::camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

/// Merged configuration from defaults, user config, and project config.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub corpus: CorpusLayout,
    pub cache: CacheConfig,
    pub pdf: PdfConfig,
    pub terms: crate::terms::Terms,
}

/// Path templates resolved against the corpus root. `{id}` is substituted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CorpusLayout {
    /// Template for a summary document.
    pub summaries: String,
    /// Candidate templates for converted Markdown, tried in order.
    pub markdown: Vec<String>,
    /// Template for the canonical PDF.
    pub pdf: String,
}

impl Default for CorpusLayout {
    fn default() -> Self {
        Self {
            summaries: "summaries/{id}.yaml".to_owned(),
            markdown: vec!["md/{id}.md".to_owned(), "md/{id}/{id}.md".to_owned()],
            pdf: "pdfs/{id}.pdf".to_owned(),
        }
    }
}

/// Extraction cache placement.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CacheConfig {
    /// Cache root. `None` uses the platform cache directory. A relative path
    /// resolves against the corpus root.
    pub root: Option<String>,
}

/// PDF extraction settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PdfConfig {
    pub ocr: OcrConfig,
}

/// OCR fallback settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OcrConfig {
    /// Whether OCR fallback is permitted at all.
    pub enabled: bool,
    /// Render resolution for OCR, in dots per inch.
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
    let mut loader = librebar::config::ConfigLoader::new("receipts").with_project_search(&search);
    if let Some(path) = explicit {
        loader = loader.with_file(to_utf8(path)?);
    }
    let (config, sources) = loader
        .load::<Config>()
        .map_err(|error| anyhow!("failed to load receipts configuration: {error}"))?;
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

/// Reject templates that could escape the corpus root.
fn validate(config: &Config) -> Result<()> {
    if config.corpus.markdown.is_empty() {
        bail!("corpus.markdown must list at least one template");
    }
    let mut templates = vec![&config.corpus.summaries, &config.corpus.pdf];
    templates.extend(&config.corpus.markdown);
    for template in templates {
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
    if config.pdf.ocr.dpi == 0 {
        bail!("pdf.ocr.dpi must be greater than zero");
    }
    if config.pdf.ocr.lang.is_empty() {
        bail!("pdf.ocr.lang must not be empty");
    }
    if config.pdf.ocr.page_segmentation_mode > 13 {
        bail!("pdf.ocr.page_segmentation_mode must be 0–13");
    }
    config.terms.validate()?;
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
