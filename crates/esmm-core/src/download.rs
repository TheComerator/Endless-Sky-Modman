//! Streaming plugin downloads with progress, cancel, a size cap and a SHA-256 of the bytes.

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

/// Streams `url` into `dest_file`, replacing it only once the whole download has succeeded.
pub fn download_to(
    url: &str,
    dest_file: &Path,
    max_bytes: u64,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> Result<Downloaded, DownloadError> {
    tracing::info!(url, "download starting");
    let result = download_to_inner(url, dest_file, max_bytes, progress, cancel);
    match &result {
        Ok(d) => tracing::info!(url, bytes = d.bytes, sha256 = d.sha256, "download finished"),
        Err(DownloadError::Cancelled) => tracing::info!(url, "download cancelled"),
        Err(e) => tracing::warn!(url, error = %e, "download failed"),
    }
    result
}

fn download_to_inner(
    url: &str,
    dest_file: &Path,
    max_bytes: u64,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> Result<Downloaded, DownloadError> {
    if !url.starts_with("https://") {
        return Err(DownloadError::NotHttps(url.to_string()));
    }
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

    let dir = match dest_file.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    // Dropped (and deleted) on any error, so a failed download never touches `dest_file`.
    let mut tmp = tempfile::Builder::new()
        .prefix(".esmm-download-")
        .tempfile_in(dir)?;
    let mut out = BufWriter::new(tmp.as_file_mut());
    let got = copy_limited(&mut reader, &mut out, total, max_bytes, progress, cancel)?;
    out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    tmp.persist(dest_file).map_err(|e| e.error)?;
    Ok(got)
}

fn copy_limited(
    reader: &mut impl Read,
    out: &mut impl Write,
    total: Option<u64>,
    max_bytes: u64,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> Result<Downloaded, DownloadError> {
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

    /// Returns at most 100 bytes per read, like a slow network body with no Content-Length.
    struct Chunked<'a>(&'a [u8]);

    impl Read for Chunked<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = buf.len().min(100).min(self.0.len());
            buf[..n].copy_from_slice(&self.0[..n]);
            self.0 = &self.0[n..];
            Ok(n)
        }
    }

    fn copy(
        data: &[u8],
        max_bytes: u64,
        progress: &mut dyn FnMut(u64, Option<u64>),
        cancel: &AtomicBool,
    ) -> (Result<Downloaded, DownloadError>, Vec<u8>) {
        let mut out = Vec::new();
        let got = copy_limited(
            &mut Chunked(data),
            &mut out,
            None,
            max_bytes,
            progress,
            cancel,
        );
        (got, out)
    }

    #[test]
    fn size_cap_applies_without_content_length() {
        let data = vec![7u8; 1000];
        let none = AtomicBool::new(false);
        let (got, _) = copy(&data, 999, &mut |_, _| {}, &none);
        assert!(matches!(got, Err(DownloadError::TooLarge(999))));

        let (got, out) = copy(&data, 1000, &mut |_, _| {}, &none);
        let got = got.unwrap();
        assert_eq!(got.bytes, 1000);
        assert_eq!(out, data);
        assert_eq!(got.sha256, hex(&Sha256::digest(&data)));
    }

    #[test]
    fn cancel_stops_partway() {
        let data = vec![1u8; 1000];
        let cancel = AtomicBool::new(false);
        let mut seen = Vec::new();
        let (got, out) = copy(
            &data,
            u64::MAX,
            &mut |done, _| {
                seen.push(done);
                if done >= 300 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
            &cancel,
        );
        assert!(matches!(got, Err(DownloadError::Cancelled)));
        assert_eq!(seen, [0, 100, 200, 300]);
        assert_eq!(out.len(), 300);
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

        // Failed downloads leave an existing file and the directory as they were.
        let small = download_to(url, &dest, 1000, &mut |_, _| {}, &AtomicBool::new(false));
        assert!(matches!(small, Err(DownloadError::TooLarge(1000))));
        let cancelled = download_to(url, &dest, 50 << 20, &mut |_, _| {}, &AtomicBool::new(true));
        assert!(matches!(cancelled, Err(DownloadError::Cancelled)));
        assert_eq!(std::fs::metadata(&dest).unwrap().len(), got.bytes);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);

        let fresh = dir.path().join("fresh.zip");
        let small = download_to(url, &fresh, 1000, &mut |_, _| {}, &AtomicBool::new(false));
        assert!(matches!(small, Err(DownloadError::TooLarge(1000))));
        assert!(!fresh.exists());
    }
}
