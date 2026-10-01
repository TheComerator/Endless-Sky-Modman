//! The per-plan `Fetcher` that adds progress reporting and a cancel flag the UI controls.
//!
//! `manager::plan_install` calls its `Fetcher` with a no-op progress callback and a fresh,
//! never-set cancel flag (it has no per-call progress of its own; see CLAUDE.md "Known
//! limitation"), so both have to come from the `Fetcher` itself: this wraps the real one,
//! ignores what planning passes in, and substitutes the plan's own.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use esmm_core::catalog::CatalogEntry;
use esmm_core::download::Downloaded;
use esmm_core::manager::Fetcher;

use crate::views::DownloadProgress;

/// Where progress goes: the Tauri event emitter in the app, a collector in tests.
pub type ProgressSink = Arc<dyn Fn(DownloadProgress) + Send + Sync>;

/// IPC events are cheap but not free; a fast download calls back thousands of times.
const MIN_INTERVAL: Duration = Duration::from_millis(100);

pub struct PlanFetcher {
    pub inner: Arc<dyn Fetcher + Send + Sync>,
    pub plan_id: u32,
    pub cancel: Arc<AtomicBool>,
    pub sink: ProgressSink,
}

impl Fetcher for PlanFetcher {
    fn fetch(
        &self,
        entry: &CatalogEntry,
        dest: &Path,
        _progress: &mut dyn FnMut(u64, Option<u64>),
        _cancel: &AtomicBool,
    ) -> Result<Downloaded, String> {
        let mut last: Option<Instant> = None;
        let mut report = |received: u64, total: Option<u64>| {
            let done = total == Some(received);
            if !done && last.is_some_and(|t| t.elapsed() < MIN_INTERVAL) {
                return;
            }
            last = Some(Instant::now());
            (self.sink)(DownloadProgress {
                plan_id: self.plan_id,
                catalog_name: entry.name.clone(),
                received,
                total,
            });
        };
        self.inner.fetch(entry, dest, &mut report, &self.cancel)
    }
}
