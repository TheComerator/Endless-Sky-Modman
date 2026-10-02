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
    #[error("download stalled: no data received for {0} seconds")]
    Stalled(u64),
}

/// How long a download may go without receiving a single byte before it's abandoned. Idle
/// time, not total time: a large plugin on a slow link that keeps trickling in is fine.
const STALL_TIMEOUT: Duration = Duration::from_secs(45);

/// How often a waiting read re-checks the cancel flag.
const POLL: Duration = Duration::from_millis(200);

/// Presents a blocking reader running on its own thread as a `Read` that gives up when no
/// bytes arrive for `stall`, or as soon as `cancel` is set, even though the worker may still be
/// stuck inside a socket read (ureq has no idle timeout to interrupt it). The abandoned worker
/// ends on its own once the socket errors, closes, or its channel send finds no receiver; the
/// bounded channel keeps it from buffering a whole download ahead of the consumer.
struct ChannelReader<'a> {
    chunks: std::sync::mpsc::Receiver<std::io::Result<Vec<u8>>>,
    pending: Vec<u8>,
    offset: usize,
    stall: Duration,
    cancel: &'a AtomicBool,
}

impl<'a> ChannelReader<'a> {
    fn spawn(
        mut source: impl Read + Send + 'static,
        stall: Duration,
        cancel: &'a AtomicBool,
    ) -> Self {
        let (tx, chunks) = std::sync::mpsc::sync_channel(4);
        std::thread::spawn(move || {
            loop {
                let mut buf = vec![0u8; 64 * 1024];
                match source.read(&mut buf) {
                    Ok(0) => return,
                    Ok(n) => {
                        buf.truncate(n);
                        if tx.send(Ok(buf)).is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                }
            }
        });
        ChannelReader {
            chunks,
            pending: Vec::new(),
            offset: 0,
            stall,
            cancel,
        }
    }
}

impl Read for ChannelReader<'_> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        use std::io::{Error, ErrorKind};
        use std::sync::mpsc::RecvTimeoutError;

        if self.offset >= self.pending.len() {
            let mut waited = Duration::ZERO;
            self.pending = loop {
                if self.cancel.load(Ordering::Relaxed) {
                    return Err(Error::new(ErrorKind::Interrupted, "download cancelled"));
                }
                match self.chunks.recv_timeout(POLL) {
                    Ok(chunk) => break chunk?,
                    // The worker finished: a clean end of the body.
                    Err(RecvTimeoutError::Disconnected) => return Ok(0),
                    Err(RecvTimeoutError::Timeout) => {
                        waited += POLL;
                        if waited >= self.stall {
                            return Err(Error::new(ErrorKind::TimedOut, "download stalled"));
                        }
                    }
                }
            };
            self.offset = 0;
        }
        let n = out.len().min(self.pending.len() - self.offset);
        out[..n].copy_from_slice(&self.pending[self.offset..self.offset + n]);
        self.offset += n;
        Ok(n)
    }
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
    let resp = agent.get(url).call()?;
    let total = resp.body().content_length();
    if total.is_some_and(|t| t > max_bytes) {
        return Err(DownloadError::TooLarge(max_bytes));
    }
    // ureq's limit is set past ours so that overflow surfaces as our own TooLarge error.
    // Read on a worker thread (see `ChannelReader`): ureq has no idle timeout, and a read
    // blocked on a stalled socket can't otherwise be interrupted by a stall check or a cancel.
    let mut reader = ChannelReader::spawn(
        resp.into_body()
            .into_with_config()
            .limit(max_bytes.saturating_add(1))
            .reader(),
        STALL_TIMEOUT,
        cancel,
    );

    let dir = match dest_file.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    // Dropped (and deleted) on any error, so a failed download never touches `dest_file`.
    let mut tmp = tempfile::Builder::new()
        .prefix(".esmm-download-")
        .tempfile_in(dir)?;
    let mut out = BufWriter::new(tmp.as_file_mut());
    let got = copy_limited(&mut reader, &mut out, total, max_bytes, progress, cancel).map_err(
        |e| match e {
            // `ChannelReader` reports both as io errors, since that's all `Read` can carry.
            DownloadError::Io(_) if cancel.load(Ordering::Relaxed) => DownloadError::Cancelled,
            DownloadError::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => {
                DownloadError::Stalled(STALL_TIMEOUT.as_secs())
            }
            other => other,
        },
    )?;
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

    /// Hands out `first` immediately, then blocks for `hang` like a socket that went quiet.
    struct Stalls {
        first: Option<Vec<u8>>,
        hang: Duration,
    }

    impl Read for Stalls {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            match self.first.take() {
                Some(bytes) => {
                    buf[..bytes.len()].copy_from_slice(&bytes);
                    Ok(bytes.len())
                }
                None => {
                    std::thread::sleep(self.hang);
                    Ok(0)
                }
            }
        }
    }

    #[test]
    fn a_stalled_read_times_out_instead_of_hanging() {
        let cancel = AtomicBool::new(false);
        let started = std::time::Instant::now();
        let mut reader = ChannelReader::spawn(
            Stalls {
                first: Some(b"hello".to_vec()),
                hang: Duration::from_secs(30),
            },
            Duration::from_millis(400),
            &cancel,
        );
        let mut out = Vec::new();
        let err = reader.read_to_end(&mut out).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert_eq!(out, b"hello", "bytes that did arrive are still delivered");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "didn't wait out the hang"
        );
    }

    #[test]
    fn cancel_interrupts_a_blocked_read() {
        let cancel = AtomicBool::new(false);
        let mut reader = ChannelReader::spawn(
            Stalls {
                first: None,
                hang: Duration::from_secs(30),
            },
            Duration::from_secs(30),
            &cancel,
        );
        let started = std::time::Instant::now();
        std::thread::scope(|s| {
            s.spawn(|| {
                std::thread::sleep(Duration::from_millis(300));
                cancel.store(true, Ordering::Relaxed);
            });
            let err = reader.read(&mut [0u8; 16]).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::Interrupted);
        });
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_healthy_body_passes_through_intact() {
        let cancel = AtomicBool::new(false);
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let mut reader = ChannelReader::spawn(
            std::io::Cursor::new(data.clone()),
            Duration::from_secs(5),
            &cancel,
        );
        let mut out = Vec::new();
        reader.read_to_end(&mut out).unwrap();
        assert_eq!(out, data);
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
