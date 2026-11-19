//! Content-addressed cache of raw Stats API response bodies.
//!
//! The cache is local and gitignored. It exists so a re-run does not repeat
//! HTTP calls. It is not a dataset to publish.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::IngestError;

/// Read a cached body for `url`, if one was stored.
pub fn read(cache_dir: &Path, url: &str) -> Result<Option<Vec<u8>>, IngestError> {
    let path = path_for(cache_dir, url);
    match fs::read(&path) {
        Ok(bytes) if !bytes.is_empty() => Ok(Some(bytes)),
        Ok(_) => Ok(None),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

/// Store a response body. The write is atomic within the cache directory.
pub fn write(cache_dir: &Path, url: &str, body: &[u8]) -> Result<(), IngestError> {
    let path = path_for(cache_dir, url);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, body)?;
    fs::rename(&tmp, &path)?;
    let mut url_path = path.clone();
    url_path.set_extension("url");
    fs::write(url_path, url)?;
    Ok(())
}

/// Cache path for a URL. The key is the SHA-256 of the URL, not of the body.
pub fn path_for(cache_dir: &Path, url: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let digest = hasher.finalize();
    let hex = hex_encode(&digest);
    cache_dir.join(&hex[..2]).join(format!("{hex}.json"))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_roundtrip_and_stable_key() {
        let dir = tempfile::tempdir().unwrap();
        let url = "https://statsapi.mlb.com/api/v1/sports";
        assert!(read(dir.path(), url).unwrap().is_none());
        write(dir.path(), url, br#"{"ok":true}"#).unwrap();
        assert_eq!(read(dir.path(), url).unwrap().unwrap(), br#"{"ok":true}"#);
        assert_eq!(path_for(dir.path(), url), path_for(dir.path(), url));
        assert_ne!(
            path_for(dir.path(), url),
            path_for(dir.path(), &format!("{url}&season=2025"))
        );
    }
}
