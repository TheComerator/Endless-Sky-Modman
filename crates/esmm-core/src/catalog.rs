//! The official plugin index from `endless-sky/endless-sky-plugins`, plus
//! Design Decision I: an on-disk cache (catalog body + ETag, and icons by URL)
//! so the app works offline and doesn't re-download unchanged data.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ureq::Agent;

pub const CATALOG_URL: &str = "https://raw.githubusercontent.com/endless-sky/endless-sky-plugins/master/generated/plugins.json";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub name: String,
    pub authors: String,
    pub homepage: String,
    pub license: String,
    /// Free-form (tags, commit SHAs, ...). Compare by equality only, never order.
    pub version: String,
    pub short_description: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Direct download for `version`. Not always GitHub.
    pub url: String,
    /// One index entry spells this `iconURL`.
    #[serde(default, alias = "iconURL")]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub autoupdate: Option<Autoupdate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Autoupdate {
    /// `tag` or `commit`.
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("failed to download the plugin catalog: {0}")]
    Http(#[from] ureq::Error),
    #[error("plugin catalog is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("failed to read or write the catalog cache: {0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Message(String),
}

pub fn parse(json: &str) -> Result<Vec<CatalogEntry>, CatalogError> {
    Ok(serde_json::from_str(json)?)
}

pub fn fetch() -> Result<Vec<CatalogEntry>, CatalogError> {
    let body = ureq::get(CATALOG_URL).call()?.body_mut().read_to_string()?;
    parse(&body)
}

// ---------------------------------------------------------------------------
// Decision I: catalog cache with ETag, and an icon cache keyed by URL.
// ---------------------------------------------------------------------------

/// Hard ceiling on any single HTTP body this module reads, regardless of what it's
/// fetching. The catalog is ~200 KB today; this leaves generous room for growth.
/// Icon downloads additionally self-enforce the tighter `ICON_CACHE_MAX_BYTES`
/// below after the fact (see `cached_icon_with`), since the trait has no way to
/// pass a per-call limit in.
const HTTP_FETCH_MAX_BYTES: u64 = 20 * 1024 * 1024;

/// Icon URLs are versioned (they embed the plugin version), so a single icon is
/// expected to be small; this just stops a misbehaving host from filling the disk.
const ICON_CACHE_MAX_BYTES: u64 = 5 * 1024 * 1024;

const CATALOG_BODY_FILE: &str = "catalog-body.json";
const CATALOG_META_FILE: &str = "catalog-meta.json";
const ICONS_SUBDIR: &str = "icons";

/// Minimal seam for injecting HTTP in tests. The real implementation (`UreqHttpGet`)
/// is used by every public no-arg-injection function (`fetch_cached`, `cached_icon`).
pub trait HttpGet {
    fn get(&self, url: &str, if_none_match: Option<&str>) -> Result<HttpResponse, String>;
}

pub struct HttpResponse {
    pub status: u16,
    pub etag: Option<String>,
    pub body: Vec<u8>,
}

struct UreqHttpGet;

impl HttpGet for UreqHttpGet {
    fn get(&self, url: &str, if_none_match: Option<&str>) -> Result<HttpResponse, String> {
        if !url.starts_with("https://") {
            return Err(format!("refusing to fetch a non-HTTPS url: {url}"));
        }
        let agent: Agent = Agent::config_builder()
            .https_only(true)
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(60)))
            .build()
            .into();
        let mut req = agent.get(url);
        if let Some(etag) = if_none_match {
            req = req.header("If-None-Match", etag);
        }
        // `http_status_as_error` (default true) only turns 4xx/5xx into `Err`; a 304
        // reaches us here as an ordinary `Ok` response (confirmed in ureq 3.4.2's
        // src/run.rs: `is_err = status.is_client_error() || status.is_server_error()`),
        // which is what lets us branch on it below instead of catching it as an error.
        let mut resp = req.call().map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let etag = resp
            .headers()
            .get(ureq::http::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let body = resp
            .body_mut()
            .with_config()
            .limit(HTTP_FETCH_MAX_BYTES)
            .read_to_vec()
            .map_err(|e| e.to_string())?;
        Ok(HttpResponse { status, etag, body })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogFetch {
    pub entries: Vec<CatalogEntry>,
    pub source: FetchSource,
    pub fetched_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchSource {
    /// A 200 response was parsed and the cache was updated.
    Fresh,
    /// A 304 response: the cached body (still parseable) was reused as-is.
    NotModified,
    /// The network/HTTP request failed; falling back to the cached body.
    Offline { error: String },
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CacheMeta {
    etag: Option<String>,
    fetched_at_unix: u64,
}

fn body_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(CATALOG_BODY_FILE)
}

fn meta_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(CATALOG_META_FILE)
}

/// Reads the cached body + metadata. Anything short of "a body that parses as a
/// catalog and a meta file that parses as JSON" is treated as no cache at all
/// (a corrupt cache must never crash or wedge the app; it's just discarded).
fn read_cache(cache_dir: &Path) -> Option<(Vec<CatalogEntry>, CacheMeta)> {
    let body = std::fs::read_to_string(body_path(cache_dir)).ok()?;
    let entries = parse(&body).ok()?;
    let meta_text = std::fs::read_to_string(meta_path(cache_dir)).ok()?;
    let meta: CacheMeta = serde_json::from_str(&meta_text).ok()?;
    Some((entries, meta))
}

/// Writes the body and its metadata as two atomic writes. Only called after the
/// body has been parsed successfully, so a good cache is never overwritten with
/// something unparseable.
fn write_cache(
    cache_dir: &Path,
    body: &str,
    etag: Option<&str>,
    fetched_at: SystemTime,
) -> Result<(), CatalogError> {
    crate::files::write_atomic(&body_path(cache_dir), body.as_bytes())?;
    let meta = CacheMeta {
        etag: etag.map(str::to_string),
        fetched_at_unix: unix_secs(fetched_at),
    };
    let json = serde_json::to_vec_pretty(&meta)?;
    crate::files::write_atomic(&meta_path(cache_dir), &json)?;
    Ok(())
}

fn unix_secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn unix_to_system(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

/// Fetches the catalog through `cache_dir`'s ETag cache. See `fetch_cached_with`
/// for the full behavior; this just wires in the real ureq-backed HTTP client.
pub fn fetch_cached(cache_dir: &Path) -> Result<CatalogFetch, CatalogError> {
    fetch_cached_with(&UreqHttpGet, cache_dir)
}

/// Core logic, generic over `HttpGet` so tests never touch the network.
///
/// - No cache, request fails: error (nothing to fall back to).
/// - No cache or stale cache, 200: parse; on success, save body+ETag and return
///   `Fresh`. On a parse failure the (possibly nonexistent) cache is left alone
///   and this returns `Err` — a 200 with a broken body is a catalog-source bug,
///   worth surfacing distinctly from a network outage, not something to paper
///   over as `Offline`.
/// - Cache present, 304: reparse the cached body (still known-good) and return
///   `NotModified`.
/// - Request fails (network/HTTP error) with a cache present: fall back to the
///   cached body as `Offline { error }`. Without a cache: error.
pub fn fetch_cached_with(
    http: &dyn HttpGet,
    cache_dir: &Path,
) -> Result<CatalogFetch, CatalogError> {
    let result = fetch_cached_with_inner(http, cache_dir);
    match &result {
        Ok(fetch) => match &fetch.source {
            FetchSource::Fresh => {
                tracing::info!(entries = fetch.entries.len(), "catalog fetch: fresh")
            }
            FetchSource::NotModified => tracing::info!("catalog fetch: not modified"),
            FetchSource::Offline { error } => {
                tracing::warn!(error, "catalog fetch: offline, using cache")
            }
        },
        Err(e) => tracing::error!(error = %e, "catalog fetch failed"),
    }
    result
}

fn fetch_cached_with_inner(
    http: &dyn HttpGet,
    cache_dir: &Path,
) -> Result<CatalogFetch, CatalogError> {
    let cached = read_cache(cache_dir);
    let if_none_match = cached.as_ref().and_then(|(_, meta)| meta.etag.as_deref());

    match http.get(CATALOG_URL, if_none_match) {
        Ok(resp) if resp.status == 304 => match cached {
            Some((entries, meta)) => Ok(CatalogFetch {
                entries,
                source: FetchSource::NotModified,
                fetched_at: unix_to_system(meta.fetched_at_unix),
            }),
            // A 304 implies the server recognized our ETag, which we only ever
            // send when we have a cache; getting here regardless means there's
            // nothing to serve.
            None => Err(CatalogError::Message(
                "server returned 304 Not Modified but no local cache exists".into(),
            )),
        },
        Ok(resp) if resp.status == 200 => {
            let body = String::from_utf8(resp.body)
                .map_err(|e| CatalogError::Message(format!("catalog body is not utf-8: {e}")))?;
            let entries = parse(&body)?;
            let now = SystemTime::now();
            write_cache(cache_dir, &body, resp.etag.as_deref(), now)?;
            Ok(CatalogFetch {
                entries,
                source: FetchSource::Fresh,
                fetched_at: now,
            })
        }
        Ok(resp) => {
            let error = format!("unexpected HTTP status {}", resp.status);
            match cached {
                Some((entries, meta)) => Ok(CatalogFetch {
                    entries,
                    source: FetchSource::Offline { error },
                    fetched_at: unix_to_system(meta.fetched_at_unix),
                }),
                None => Err(CatalogError::Message(error)),
            }
        }
        Err(error) => match cached {
            Some((entries, meta)) => Ok(CatalogFetch {
                entries,
                source: FetchSource::Offline { error },
                fetched_at: unix_to_system(meta.fetched_at_unix),
            }),
            None => Err(CatalogError::Message(error)),
        },
    }
}

fn icons_dir(cache_dir: &Path) -> PathBuf {
    cache_dir.join(ICONS_SUBDIR)
}

/// Icon URLs embed the plugin version, so the same URL always means the same
/// bytes: cache by URL forever, keyed by its sha256 hex digest. A plain image
/// extension is kept on the filename (nice for debugging / opening by hand);
/// anything else gets no extension.
fn icon_path(icons_dir: &Path, url: &str) -> PathBuf {
    let digest = hex(&Sha256::digest(url.as_bytes()));
    match plain_image_extension(url) {
        Some(ext) => icons_dir.join(format!("{digest}.{ext}")),
        None => icons_dir.join(digest),
    }
}

fn plain_image_extension(url: &str) -> Option<&'static str> {
    let without_query = url.split(['?', '#']).next().unwrap_or(url);
    let ext = Path::new(without_query)
        .extension()?
        .to_str()?
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("png"),
        "jpg" => Some("jpg"),
        "jpeg" => Some("jpeg"),
        "gif" => Some("gif"),
        "webp" => Some("webp"),
        _ => None,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Returns the local path to `url`'s cached icon, downloading it first if needed.
pub fn cached_icon(cache_dir: &Path, url: &str) -> Result<PathBuf, CatalogError> {
    cached_icon_with(&UreqHttpGet, cache_dir, url)
}

/// Core logic, generic over `HttpGet` so tests never touch the network.
pub fn cached_icon_with(
    http: &dyn HttpGet,
    cache_dir: &Path,
    url: &str,
) -> Result<PathBuf, CatalogError> {
    if !url.starts_with("https://") {
        return Err(CatalogError::Message(format!(
            "refusing to cache a non-HTTPS icon url: {url}"
        )));
    }
    let icons_dir = icons_dir(cache_dir);
    let path = icon_path(&icons_dir, url);
    if path.exists() {
        return Ok(path);
    }
    let resp = http.get(url, None).map_err(CatalogError::Message)?;
    if resp.status != 200 {
        return Err(CatalogError::Message(format!(
            "unexpected HTTP status {} fetching icon",
            resp.status
        )));
    }
    if resp.body.len() as u64 > ICON_CACHE_MAX_BYTES {
        return Err(CatalogError::Message(format!(
            "icon is larger than the {ICON_CACHE_MAX_BYTES} byte limit"
        )));
    }
    crate::files::write_atomic(&path, &resp.body)?;
    Ok(path)
}

/// Deletes cached icons whose URL is no longer in `keep_urls` (e.g. after a
/// fresh catalog fetch drops or changes a plugin's icon). Never touches anything
/// outside `cache_dir`'s icons subdirectory.
pub fn prune_icons(cache_dir: &Path, keep_urls: &[&str]) -> Result<(), CatalogError> {
    let icons_dir = icons_dir(cache_dir);
    let keep: HashSet<PathBuf> = keep_urls
        .iter()
        .map(|url| icon_path(&icons_dir, url))
        .collect();
    let entries = match std::fs::read_dir(&icons_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_file() && !keep.contains(&path) {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}
