//! File-based logging (decision K's logging addition).
//!
//! Unifies both ecosystems other crates in this workspace might use into one file:
//! `tracing_log::LogTracer` redirects any `log` crate record (from Tauri/webview internals)
//! into a `tracing::Event`, and the one global `tracing_subscriber` below - the only
//! consumer registered - writes every event, from either source, to the same file.
//!
//! Rotates daily (`tracing_appender::rolling::daily`); old files aren't pruned automatically,
//! which is an accepted v1 limitation for a single-user desktop app's log volume.
//!
//! Started from [`crate::shell::Deps::production`]'s setup, so anything logged before that
//! runs (essentially none of this app's own code - only Tauri's own bootstrap) isn't
//! captured. Call [`init`] at most once; it returns `None` and logs nothing on a second call
//! rather than panicking, so a hot-reloaded `setup` (shouldn't happen, but isn't relied on
//! not to) fails safe.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

/// Sets up the global log sink and returns its background-writer guard, which the caller
/// must keep alive for the app's lifetime (dropping it stops the writer thread).
pub fn init(log_dir: &Path) -> Option<WorkerGuard> {
    if std::fs::create_dir_all(log_dir).is_err() {
        return None;
    }
    let file_appender = tracing_appender::rolling::daily(log_dir, "esmm.log");
    let (writer, guard) = tracing_appender::non_blocking(file_appender);

    let subscriber = tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .finish();
    if tracing::subscriber::set_global_default(subscriber).is_err() {
        // Already initialized (e.g. a second `setup` call) - not a hard error, just a no-op.
        return None;
    }
    // Bridges `log::` macro calls (Tauri's own internals, webview) into the subscriber above.
    let _ = tracing_log::LogTracer::init();
    Some(guard)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    // The only test here: `init` sets the process-global tracing subscriber, which can only
    // succeed once per process, and Rust's test harness runs every `#[test]` in one process.
    // A second test calling `init` would just observe the documented "already set" `None`.
    #[test]
    fn init_creates_the_log_dir_and_writes_an_event() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("logs");
        let guard = init(&log_dir).expect("first call in this process should succeed");

        tracing::info!(
            marker = "init_creates_the_log_dir_and_writes_an_event",
            "test event"
        );
        drop(guard); // flushes the non-blocking writer

        let entries: Vec<_> = fs::read_dir(&log_dir).unwrap().collect();
        assert_eq!(entries.len(), 1, "exactly one daily log file");
        let content = fs::read_to_string(entries[0].as_ref().unwrap().path()).unwrap();
        assert!(
            content.contains("init_creates_the_log_dir_and_writes_an_event"),
            "log file should contain the event we just wrote: {content:?}"
        );
    }
}
