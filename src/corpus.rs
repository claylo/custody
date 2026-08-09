//! Corpus root and template-driven source resolution.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::{self, CorpusLayout, Discovered, SourceTemplates};
use crate::evidence::DEFAULT_SOURCE;

/// A resolved corpus: root directory, layout templates, and cache root.
#[derive(Debug, Clone)]
pub struct Corpus {
    root: PathBuf,
    layout: CorpusLayout,
    cache_root: PathBuf,
    config_file: Option<PathBuf>,
    terms: crate::terms::Terms,
    ocr_config: crate::config::OcrConfig,
}

impl Corpus {
    /// Discover configuration from `start` and resolve the corpus.
    pub fn discover_from(start: &Path, explicit_config: Option<&Path>) -> Result<Self> {
        Self::from_discovered(config::load(start, explicit_config)?)
    }

    /// Build a corpus from an already-resolved configuration.
    pub fn from_discovered(discovered: Discovered) -> Result<Self> {
        let Discovered {
            config,
            root,
            config_file,
        } = discovered;
        let cache_root = match config.cache.root.as_deref() {
            Some(value) => {
                let candidate = PathBuf::from(value);
                if candidate.is_absolute() {
                    candidate
                } else {
                    root.join(candidate)
                }
            }
            None => config::platform_cache_root()?,
        };
        Ok(Self {
            root,
            layout: config.corpus,
            cache_root,
            config_file,
            terms: config.terms,
            ocr_config: config.pdf.ocr,
        })
    }

    /// Vocabulary this corpus uses for a claim.
    #[must_use]
    pub const fn terms(&self) -> &crate::terms::Terms {
        &self.terms
    }

    /// OCR fallback settings for this corpus.
    #[must_use]
    pub const fn ocr_config(&self) -> &crate::config::OcrConfig {
        &self.ocr_config
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }

    /// Path of the discovered project config file, if any.
    #[must_use]
    pub fn config_file(&self) -> Option<&Path> {
        self.config_file.as_deref()
    }

    /// Directory holding summary documents, derived from the template.
    #[must_use]
    pub fn summaries_dir(&self) -> PathBuf {
        let prefix = self
            .layout
            .summaries
            .split("{id}")
            .next()
            .unwrap_or_default();
        let trimmed = prefix.trim_end_matches('/');
        if trimmed.is_empty() {
            self.root.clone()
        } else {
            self.root.join(trimmed)
        }
    }

    pub fn summary_path(&self, id: &str) -> Result<PathBuf> {
        validate_id(id)?;
        Ok(self.root.join(render(&self.layout.summaries, id)))
    }

    /// Source names this corpus declares templates for.
    #[must_use]
    pub fn source_names(&self) -> Vec<String> {
        self.layout.sources.keys().cloned().collect()
    }

    pub fn markdown_candidates(&self, id: &str) -> Result<Vec<PathBuf>> {
        validate_id(id)?;
        Ok(self
            .templates(DEFAULT_SOURCE)?
            .markdown
            .iter()
            .map(|template| self.root.join(render(template, id)))
            .collect())
    }

    pub fn pdf_path(&self, id: &str) -> Result<PathBuf> {
        validate_id(id)?;
        Ok(self
            .root
            .join(render(&self.templates(DEFAULT_SOURCE)?.pdf, id)))
    }

    fn templates(&self, source: &str) -> Result<&SourceTemplates> {
        self.layout
            .sources
            .get(source)
            .with_context(|| format!("corpus declares no templates for source {source:?}"))
    }
}

fn render(template: &str, id: &str) -> String {
    template.replace("{id}", id)
}

fn validate_id(id: &str) -> Result<()> {
    let valid = id.len() >= 3
        && !id.starts_with('-')
        && !id.ends_with('-')
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if !valid {
        bail!("invalid summary ID {id:?}");
    }
    Ok(())
}
