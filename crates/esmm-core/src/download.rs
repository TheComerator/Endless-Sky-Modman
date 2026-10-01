//! Streaming plugin downloads with progress, cancel, a size cap and a SHA-256 of the bytes.

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};
use ureq::Agent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Downloaded {
    pub bytes: u64,
    /// Lowercase hex.
    pub sha256: String,
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("refusing to download over a non-HTTPS URL: {0}")]
    NotHttps(String),
    #[error("download failed: {0}")]
    Http(#[from] ureq::Error),
    #[error("failed to write the download: {0}")]
    Io(#[from] std::io::Error),
    #[error("download is larger than the {0} byte limit")]
    TooLarge(u64),
    #[error("download cancelled")]
    Cancelled,
}

pub fn download_to(
    url: &str,
    dest_file: &Path,
    max_bytes: u64,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> Result<Downloaded, DownloadError> {
    if !url.starts_with("https://") {
        return Err(DownloadError::NotHttps(url.to_string()));
    }
    let result = stream(url, dest_file, max_bytes, progress, cancel);
    if result.is_err() {
        let _ = std::fs::remove_file(dest_file);
    }
    result
}

fn stream(
    url: &str,
    dest_file: &Path,
    max_bytes: u64,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> Result<Downloaded, DownloadError> {
    // No body timeout: large plugins on slow links must still finish. Cancel covers the rest.
    let agent: Agent = Agent::config_builder()
        .https_only(true)
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .build()
        .into();
    let mut resp = agent.get(url).call()?;
    let total = resp.body().content_length();
    if total.is_some_and(|t| t > max_bytes) {
        return Err(DownloadError::TooLarge(max_bytes));
    }
    // ureq's limit is set past ours so that overflow surfaces as our own TooLarge error.
    let mut reader = resp
        .body_mut()
        .with_config()
        .limit(max_bytes.saturating_add(1))
        .reader();

    let mut out = BufWriter::new(File::create(dest_file)?);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    progress(0, total);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(DownloadError::Cancelled);
        }
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        done += n as u64;
        if done > max_bytes {
            return Err(DownloadError::TooLarge(max_bytes));
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        progress(done, total);
    }
    out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    Ok(Downloaded {
        bytes: done,
        sha256: hex(&hasher.finalize()),
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn try_download(url: &str, dest: &Path) -> Result<Downloaded, DownloadError> {
        download_to(url, dest, 1 << 30, &mut |_, _| {}, &AtomicBool::new(false))
    }

    #[test]
    fn rejects_non_https_urls() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("x.zip");
        for url in [
            "http://github.com/a/b.zip",
            "ftp://example.com/a.zip",
            "file:///etc/passwd",
            "HTTPS://example.com/a.zip",
            "github.com/a/b.zip",
        ] {
            assert!(
                matches!(try_download(url, &dest), Err(DownloadError::NotHttps(_))),
                "{url}"
            );
        }
        assert!(!dest.exists());
    }

    #[test]
    fn hex_is_lowercase() {
        assert_eq!(hex(&[0x00, 0xab, 0xff]), "00abff");
    }

    #[test]
    #[ignore = "hits the network"]
    fn downloads_a_redirecting_release_asset() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("ship.merging.zip");
        let url = "https://github.com/zuckung/endless-sky-plugins/releases/download/v1.0.8-ship.merging/ship.merging.zip";
        let mut calls = 0;
        let got = download_to(
            url,
            &dest,
            50 << 20,
            &mut |_, _| calls += 1,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(got.bytes > 1 << 20);
        assert_eq!(std::fs::metadata(&dest).unwrap().len(), got.bytes);
        assert_eq!(got.sha256.len(), 64);
        assert!(calls > 1);

        let small = download_to(url, &dest, 1000, &mut |_, _| {}, &AtomicBool::new(false));
        assert!(matches!(small, Err(DownloadError::TooLarge(1000))));
        assert!(!dest.exists());

        let cancelled = download_to(url, &dest, 50 << 20, &mut |_, _| {}, &AtomicBool::new(true));
        assert!(matches!(cancelled, Err(DownloadError::Cancelled)));
        assert!(!dest.exists());
    }
}
