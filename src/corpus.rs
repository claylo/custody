//! Corpus root and template-driven source resolution.

use std::{
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use crate::config::{self, CorpusLayout, Discovered, SourceTemplates};
use crate::evidence::DEFAULT_SOURCE;

const MAX_CORPUS_TEXT_BYTES: u64 = 64 * 1024 * 1024;

pub(crate) enum ResolvedCorpusFile {
    Contained(PathBuf),
    Outside(PathBuf),
}

/// A resolved corpus: root directory, layout templates, and cache root.
#[derive(Debug, Clone)]
pub struct Corpus {
    root: PathBuf,
    layout: CorpusLayout,
    cache_root: PathBuf,
    config_file: Option<PathBuf>,
    terms: crate::terms::Terms,
    pdf_config: crate::config::PdfConfig,
    coverage_config: crate::config::CoverageConfig,
    sections_config: crate::config::SectionsConfig,
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
            Some(value) => resolve_cache_root(&root, value)?,
            None => config::platform_cache_root()?,
        };
        Ok(Self {
            root,
            layout: config.corpus,
            cache_root,
            config_file,
            terms: config.terms,
            pdf_config: config.pdf,
            coverage_config: config.coverage,
            sections_config: config.sections,
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
        &self.pdf_config.ocr
    }

    /// PDF tool and OCR settings for this corpus.
    #[must_use]
    pub const fn pdf_config(&self) -> &crate::config::PdfConfig {
        &self.pdf_config
    }

    /// Claim-token coverage settings for this corpus.
    #[must_use]
    pub const fn coverage_config(&self) -> &crate::config::CoverageConfig {
        &self.coverage_config
    }

    /// Weak-section settings for this corpus.
    #[must_use]
    pub const fn sections_config(&self) -> &crate::config::SectionsConfig {
        &self.sections_config
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }

    /// Whether the configured cache is controlled by corpus content.
    #[must_use]
    pub fn cache_is_corpus_local(&self) -> bool {
        self.cache_root.starts_with(&self.root)
    }

    pub(crate) fn resolve_contained_file(&self, path: &Path) -> Result<ResolvedCorpusFile> {
        let resolved = path
            .canonicalize()
            .with_context(|| format!("failed to resolve {}", path.display()))?;
        if !resolved.starts_with(&self.root) {
            return Ok(ResolvedCorpusFile::Outside(resolved));
        }
        if !resolved
            .metadata()
            .with_context(|| format!("failed to inspect {}", resolved.display()))?
            .is_file()
        {
            bail!("{} is not a regular file", resolved.display());
        }
        Ok(ResolvedCorpusFile::Contained(resolved))
    }

    pub(crate) fn read_contained_text(&self, path: &Path) -> Result<String> {
        let resolved = match self.resolve_contained_file(path)? {
            ResolvedCorpusFile::Contained(resolved) => resolved,
            ResolvedCorpusFile::Outside(resolved) => bail!(
                "{} resolves outside the corpus to {}",
                path.display(),
                resolved.display()
            ),
        };
        let file = fs::File::open(&resolved)
            .with_context(|| format!("failed to open {}", resolved.display()))?;
        let length = file
            .metadata()
            .with_context(|| format!("failed to inspect {}", resolved.display()))?
            .len();
        if length > MAX_CORPUS_TEXT_BYTES {
            bail!(
                "{} exceeds the {MAX_CORPUS_TEXT_BYTES}-byte corpus text limit",
                resolved.display()
            );
        }
        let mut source = String::new();
        file.take(MAX_CORPUS_TEXT_BYTES + 1)
            .read_to_string(&mut source)
            .with_context(|| format!("failed to read {} as UTF-8", resolved.display()))?;
        if u64::try_from(source.len()).unwrap_or(u64::MAX) > MAX_CORPUS_TEXT_BYTES {
            bail!(
                "{} exceeds the {MAX_CORPUS_TEXT_BYTES}-byte corpus text limit",
                resolved.display()
            );
        }
        Ok(source)
    }

    /// Path of the discovered project config file, if any.
    #[must_use]
    pub fn config_file(&self) -> Option<&Path> {
        self.config_file.as_deref()
    }

    /// The raw summary template string (e.g. `summaries/{id}.yaml`).
    #[must_use]
    pub fn summary_template(&self) -> &str {
        &self.layout.summaries
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
        self.markdown_candidates_for(id, DEFAULT_SOURCE)
    }

    pub fn markdown_candidates_for(&self, id: &str, source: &str) -> Result<Vec<PathBuf>> {
        validate_id(id)?;
        Ok(self
            .templates(source)?
            .markdown
            .iter()
            .map(|template| self.root.join(render(template, id)))
            .collect())
    }

    pub fn pdf_path(&self, id: &str) -> Result<PathBuf> {
        self.pdf_path_for(id, DEFAULT_SOURCE)
    }

    pub fn pdf_path_for(&self, id: &str, source: &str) -> Result<PathBuf> {
        validate_id(id)?;
        Ok(self.root.join(render(&self.templates(source)?.pdf, id)))
    }

    fn templates(&self, source: &str) -> Result<&SourceTemplates> {
        self.layout
            .sources
            .get(source)
            .with_context(|| format!("corpus declares no templates for source {source:?}"))
    }
}

fn resolve_cache_root(root: &Path, declared: &str) -> Result<PathBuf> {
    config::validate_cache_root(declared)?;
    let candidate = root.join(declared);
    let mut existing = candidate.as_path();
    let mut missing = Vec::<OsString>::new();

    loop {
        match fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = existing.file_name().with_context(|| {
                    format!(
                        "cache.root has no existing ancestor: {}",
                        candidate.display()
                    )
                })?;
                missing.push(name.to_os_string());
                existing = existing.parent().with_context(|| {
                    format!(
                        "cache.root has no existing ancestor: {}",
                        candidate.display()
                    )
                })?;
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to inspect cache.root ancestor {}",
                        existing.display()
                    )
                });
            }
        }
    }

    let mut resolved = existing.canonicalize().with_context(|| {
        format!(
            "failed to resolve cache.root ancestor {}",
            existing.display()
        )
    })?;
    if !resolved.starts_with(root) {
        bail!(
            "cache.root resolves outside the corpus root: {}",
            resolved.display()
        );
    }
    for component in missing.iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
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
