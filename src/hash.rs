use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

/// Return the lowercase SHA-256 digest of `bytes`.
#[must_use]
pub fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Return the lowercase SHA-256 digest of the raw bytes at `path`.
pub fn sha256_file(path: &Path) -> Result<String> {
    let file = File::open(path)
        .with_context(|| format!("failed to open {} for hashing", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();

    loop {
        let read = reader
            .read(&mut buffer)
            .with_context(|| format!("failed to read {} for hashing", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}
