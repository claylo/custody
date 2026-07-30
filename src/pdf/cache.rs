use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::hash::sha256_bytes;

/// All settings and versions that can affect OCR output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OcrProfile {
    pub name: String,
    pub language: String,
    pub dpi: u16,
    pub page_segmentation_mode: u8,
    pub mutool_version: String,
    pub tesseract_version: String,
}

/// Manifest that makes a cached TSV self-validating.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheManifest {
    pub pdf_sha256: String,
    pub page: usize,
    pub profile: OcrProfile,
    pub toolchain_sha256: String,
}

impl CacheManifest {
    pub fn new(pdf_sha256: String, page: usize, profile: OcrProfile) -> Result<Self> {
        let toolchain_sha256 = cache_key(&profile)?;
        Ok(Self {
            pdf_sha256,
            page,
            profile,
            toolchain_sha256,
        })
    }
}

/// Persistent page-level OCR cache rooted at the configured cache root.
///
/// Defaults to the platform cache directory; see [`crate::config::CacheConfig`].
/// Entries never expire: every key component is verified before reuse, so a hit
/// is a determinism guarantee rather than only a saved subprocess.
#[derive(Debug, Clone)]
pub struct OcrCache {
    root: PathBuf,
}

impl OcrCache {
    #[must_use]
    pub const fn new(root: PathBuf) -> Self {
        Self { root }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn entry_dir(&self, manifest: &CacheManifest) -> PathBuf {
        self.root
            .join(&manifest.pdf_sha256)
            .join(&manifest.profile.name)
            .join(&manifest.toolchain_sha256)
            .join(format!("page-{:04}", manifest.page))
    }

    pub fn load(&self, expected: &CacheManifest) -> Result<Option<String>> {
        let directory = self.entry_dir(expected);
        let manifest_path = directory.join("manifest.json");
        let tsv_path = directory.join("ocr.tsv");
        if !manifest_path.is_file() || !tsv_path.is_file() {
            return Ok(None);
        }
        let stored: CacheManifest = serde_json::from_slice(
            &fs::read(&manifest_path)
                .with_context(|| format!("failed to read {}", manifest_path.display()))?,
        )
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
        if stored != *expected {
            return Ok(None);
        }
        fs::read_to_string(&tsv_path)
            .with_context(|| format!("failed to read {}", tsv_path.display()))
            .map(Some)
    }

    pub fn store(&self, manifest: &CacheManifest, tsv: &str) -> Result<()> {
        let directory = self.entry_dir(manifest);
        fs::create_dir_all(&directory)
            .with_context(|| format!("failed to create {}", directory.display()))?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let suffix = format!("{}.{}", std::process::id(), nonce);
        let tsv_temp = directory.join(format!(".ocr.tsv.{suffix}.tmp"));
        let manifest_temp = directory.join(format!(".manifest.json.{suffix}.tmp"));
        fs::write(&tsv_temp, tsv)
            .with_context(|| format!("failed to write {}", tsv_temp.display()))?;
        fs::write(&manifest_temp, serde_json::to_vec_pretty(manifest)?)
            .with_context(|| format!("failed to write {}", manifest_temp.display()))?;
        fs::rename(&tsv_temp, directory.join("ocr.tsv")).context("failed to publish OCR TSV")?;
        fs::rename(&manifest_temp, directory.join("manifest.json"))
            .context("failed to publish OCR cache manifest")?;
        Ok(())
    }
}

pub fn cache_key(profile: &OcrProfile) -> Result<String> {
    Ok(sha256_bytes(&serde_json::to_vec(profile)?))
}
