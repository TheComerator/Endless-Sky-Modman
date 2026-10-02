//! File-based logging (decision K's logging addition).
//!
//! Unifies both ecosystems other crates in this workspace might use into one file:
//! `tracing_log::LogTracer` redirects any `log` crate record (from Tauri/webview internals)
//! into a `tracing::Event`, and the one global `tracing_subscriber` below - the only
//! consumer registered - writes every event, from either source, to the same file.
//!
//! Rotates daily (`tracing_appender::rolling::daily`); [`prune_old_logs`] deletes anything
//! older than [`LOG_RETENTION`], run once at startup, so the directory doesn't grow forever
//! across a single-user desktop app's lifetime.
//!
//! Started from [`crate::shell::Deps::production`]'s setup, so anything logged before that
//! runs (essentially none of this app's own code - only Tauri's own bootstrap) isn't
//! captured. Call [`init`] at most once; it returns `None` and logs nothing on a second call
//! rather than panicking, so a hot-reloaded `setup` (shouldn't happen, but isn't relied on
//! not to) fails safe.

use std::path::Path;
use std::time::{Duration, SystemTime};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

/// How long a rotated log file is kept before [`prune_old_logs`] deletes it. Generous on
/// purpose: these are for after-the-fact debugging, not a storage-sensitive cache.
const LOG_RETENTION: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// Deletes rotated log files (named `<prefix>.<date>`, as `tracing_appender::rolling::daily`
/// writes them) last modified more than `max_age` before `now`. Each entry is pruned
/// independently on a best-effort basis: an unreadable directory, or one file that can't be
/// inspected or removed, never stops the rest, since this is housekeeping, not something the
/// app's own operation depends on.
pub fn prune_old_logs(log_dir: &Path, prefix: &str, max_age: Duration, now: SystemTime) {
    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_ours = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(prefix));
        if !is_ours {
            continue;
        }
        let age = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok());
        if age.is_some_and(|age| age > max_age) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Sets up the global log sink and returns its background-writer guard, which the caller
/// must keep alive for the app's lifetime (dropping it stops the writer thread).
pub fn init(log_dir: &Path) -> Option<WorkerGuard> {
    if std::fs::create_dir_all(log_dir).is_err() {
        return None;
    }
    prune_old_logs(log_dir, "esmm.log", LOG_RETENTION, SystemTime::now());
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

    #[test]
    fn prune_old_logs_deletes_only_old_matching_files() {
        let dir = tempfile::tempdir().unwrap();
        let now = SystemTime::now();
        let old = now - Duration::from_secs(20 * 24 * 60 * 60);
        let young = now - Duration::from_secs(24 * 60 * 60);

        let make = |name: &str, modified: SystemTime| {
            let path = dir.path().join(name);
            let f = fs::File::create(&path).unwrap();
            f.set_modified(modified).unwrap();
            path
        };

        let old_log = make("esmm.log.2026-09-01", old);
        let young_log = make("esmm.log.2026-09-20", young);
        // Older than the retention window, but not one of ours: must survive regardless.
        let unrelated = make("other.txt", old);

        prune_old_logs(dir.path(), "esmm.log", LOG_RETENTION, now);

        assert!(!old_log.exists(), "old log file should be pruned");
        assert!(young_log.exists(), "young log file should be kept");
        assert!(
            unrelated.exists(),
            "non-matching file should never be touched"
        );
    }

    #[test]
    fn prune_old_logs_on_a_missing_dir_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        prune_old_logs(&missing, "esmm.log", LOG_RETENTION, SystemTime::now());
    }
}
