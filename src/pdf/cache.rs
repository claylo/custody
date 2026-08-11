use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
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
    pub render_command: String,
    pub orientation_command: String,
    pub recognition_command: String,
}

/// Manifest that makes a cached TSV self-validating.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheManifest {
    pub pdf_sha256: String,
    pub page: usize,
    pub profile: OcrProfile,
    pub toolchain_sha256: String,
    pub render_rotation_degrees: i16,
}

impl CacheManifest {
    pub fn new(pdf_sha256: String, page: usize, profile: OcrProfile) -> Result<Self> {
        if pdf_sha256.len() != 64
            || !pdf_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            bail!("PDF SHA-256 must be 64 lowercase hexadecimal characters");
        }
        if page == 0 {
            bail!("PDF pages are one-based");
        }
        if profile.name.is_empty()
            || !profile
                .name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            bail!("OCR profile name is not a safe cache path component");
        }
        let toolchain_sha256 = cache_key(&profile)?;
        Ok(Self {
            pdf_sha256,
            page,
            profile,
            toolchain_sha256,
            render_rotation_degrees: 0,
        })
    }

    pub fn with_render_rotation(mut self, rotation: i16) -> Result<Self> {
        if !matches!(rotation, 0 | 90 | 180 | 270) {
            bail!("render rotation must be 0, 90, 180, or 270 degrees");
        }
        self.render_rotation_degrees = rotation;
        Ok(self)
    }

    fn matches_request(&self, expected: &Self) -> bool {
        matches!(self.render_rotation_degrees, 0 | 90 | 180 | 270)
            && self.pdf_sha256 == expected.pdf_sha256
            && self.page == expected.page
            && self.profile == expected.profile
            && self.toolchain_sha256 == expected.toolchain_sha256
    }
}

/// Persistent page-level OCR cache rooted at the configured cache root.
///
/// Defaults to the platform cache directory; see [`crate::config::CacheConfig`].
/// Trusted entries never expire because every key component is verified before
/// reuse. Corpus-local entries are write-only unless the operator explicitly
/// opts in to trusting them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheReadPolicy {
    /// Reuse matching entries from this cache root.
    Trusted,
    /// Permit writes but treat every existing entry as a miss.
    WriteOnly,
}

#[derive(Debug, Clone)]
pub struct OcrCache {
    root: PathBuf,
    read_policy: CacheReadPolicy,
}

impl OcrCache {
    #[must_use]
    pub const fn new(root: PathBuf) -> Self {
        Self::with_read_policy(root, CacheReadPolicy::Trusted)
    }

    #[must_use]
    pub const fn with_read_policy(root: PathBuf, read_policy: CacheReadPolicy) -> Self {
        Self { root, read_policy }
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
        if self.read_policy == CacheReadPolicy::WriteOnly {
            return Ok(None);
        }
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
        if !stored.matches_request(expected) {
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
