//! Tests for Design Decision I (catalog + icon caching) in `esmm_core::catalog`.
//! All HTTP is faked via `HttpGet`; nothing here touches the network except the
//! one `#[ignore]`d test at the bottom.

use std::cell::RefCell;

use esmm_core::catalog::{self, CatalogError, FetchSource, HttpGet, HttpResponse};

const SAMPLE_A: &str = r#"[{"name":"A","authors":"x","homepage":"https://a","license":"MIT","version":"1.0","shortDescription":"a","url":"https://a/a.zip"}]"#;
const SAMPLE_B: &str = r#"[{"name":"B","authors":"x","homepage":"https://b","license":"MIT","version":"2.0","shortDescription":"b","url":"https://b/b.zip"}]"#;

/// A scripted HTTP client: each call to `get` pops the next entry off `responses`
/// (in order) and records the request it was given.
struct FakeHttp {
    responses: RefCell<Vec<Result<HttpResponse, String>>>,
    requests: RefCell<Vec<(String, Option<String>)>>,
}

impl FakeHttp {
    fn new(responses: Vec<Result<HttpResponse, String>>) -> Self {
        FakeHttp {
            responses: RefCell::new(responses),
            requests: RefCell::new(Vec::new()),
        }
    }
}

impl HttpGet for FakeHttp {
    fn get(&self, url: &str, if_none_match: Option<&str>) -> Result<HttpResponse, String> {
        self.requests
            .borrow_mut()
            .push((url.to_string(), if_none_match.map(str::to_string)));
        let mut responses = self.responses.borrow_mut();
        assert!(!responses.is_empty(), "unexpected extra request to {url}");
        responses.remove(0)
    }
}

fn ok(status: u16, etag: Option<&str>, body: &str) -> Result<HttpResponse, String> {
    Ok(HttpResponse {
        status,
        etag: etag.map(str::to_string),
        body: body.as_bytes().to_vec(),
    })
}

#[test]
fn first_fetch_is_fresh_and_stores_body_and_etag() {
    let dir = tempfile::tempdir().unwrap();
    let http = FakeHttp::new(vec![ok(200, Some("\"v1\""), SAMPLE_A)]);

    let fetch = catalog::fetch_cached_with(&http, dir.path()).unwrap();
    assert_eq!(fetch.source, FetchSource::Fresh);
    assert_eq!(fetch.entries.len(), 1);
    assert_eq!(fetch.entries[0].name, "A");

    assert!(dir.path().join("catalog-body.json").exists());
    assert!(dir.path().join("catalog-meta.json").exists());
    assert_eq!(
        http.requests.borrow()[0],
        (catalog::CATALOG_URL.to_string(), None)
    );
}

#[test]
fn second_fetch_sends_stored_etag_and_304_reuses_entries() {
    let dir = tempfile::tempdir().unwrap();
    let http = FakeHttp::new(vec![ok(200, Some("\"v1\""), SAMPLE_A), ok(304, None, "")]);

    let first = catalog::fetch_cached_with(&http, dir.path()).unwrap();
    assert_eq!(first.source, FetchSource::Fresh);

    let second = catalog::fetch_cached_with(&http, dir.path()).unwrap();
    assert_eq!(second.source, FetchSource::NotModified);
    assert_eq!(second.entries, first.entries);

    // The second request carried the ETag the first response gave us.
    let requests = http.requests.borrow();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].1.as_deref(), Some("\"v1\""));
}

#[test]
fn new_etag_on_200_replaces_the_cache() {
    let dir = tempfile::tempdir().unwrap();
    let http = FakeHttp::new(vec![
        ok(200, Some("\"v1\""), SAMPLE_A),
        ok(200, Some("\"v2\""), SAMPLE_B),
    ]);

    catalog::fetch_cached_with(&http, dir.path()).unwrap();
    let second = catalog::fetch_cached_with(&http, dir.path()).unwrap();
    assert_eq!(second.source, FetchSource::Fresh);
    assert_eq!(second.entries[0].name, "B");

    // A third, cacheless client with no network should now see B, and send v2.
    let http2 = FakeHttp::new(vec![ok(304, None, "")]);
    let third = catalog::fetch_cached_with(&http2, dir.path()).unwrap();
    assert_eq!(third.source, FetchSource::NotModified);
    assert_eq!(third.entries[0].name, "B");
    assert_eq!(http2.requests.borrow()[0].1.as_deref(), Some("\"v2\""));
}

#[test]
fn network_error_with_cache_falls_back_offline() {
    let dir = tempfile::tempdir().unwrap();
    let http = FakeHttp::new(vec![
        ok(200, Some("\"v1\""), SAMPLE_A),
        Err("connection reset".to_string()),
    ]);

    catalog::fetch_cached_with(&http, dir.path()).unwrap();
    let second = catalog::fetch_cached_with(&http, dir.path()).unwrap();
    match second.source {
        FetchSource::Offline { error } => assert_eq!(error, "connection reset"),
        other => panic!("expected Offline, got {other:?}"),
    }
    assert_eq!(second.entries[0].name, "A");
}

#[test]
fn network_error_without_cache_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let http = FakeHttp::new(vec![Err("dns failure".to_string())]);

    let err = catalog::fetch_cached_with(&http, dir.path()).unwrap_err();
    assert!(matches!(err, CatalogError::Message(msg) if msg == "dns failure"));
}

/// Documented choice: a 200 with a body that doesn't parse as the catalog schema
/// is treated as a catalog-source bug, not a network blip. It returns `Err` and
/// leaves whatever cache existed untouched (never overwritten with garbage).
#[test]
fn invalid_json_on_200_keeps_old_cache_and_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let http = FakeHttp::new(vec![
        ok(200, Some("\"v1\""), SAMPLE_A),
        ok(200, Some("\"v2\""), "not json"),
    ]);

    catalog::fetch_cached_with(&http, dir.path()).unwrap();
    let err = catalog::fetch_cached_with(&http, dir.path());
    assert!(err.is_err());

    // The old, good cache is still there and still serves entries via a later 304.
    let http2 = FakeHttp::new(vec![ok(304, None, "")]);
    let after = catalog::fetch_cached_with(&http2, dir.path()).unwrap();
    assert_eq!(after.entries[0].name, "A");
    // Crucially the ETag on disk is still "v1", not the "v2" from the rejected body.
    assert_eq!(http2.requests.borrow()[0].1.as_deref(), Some("\"v1\""));
}

#[test]
fn corrupt_cache_body_is_treated_as_absent() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("catalog-body.json"), "{not valid json").unwrap();
    std::fs::write(
        dir.path().join("catalog-meta.json"),
        r#"{"etag":"\"v1\"","fetched_at_unix":1}"#,
    )
    .unwrap();

    // No If-None-Match should be sent, since the cache was discarded as corrupt.
    let http = FakeHttp::new(vec![ok(200, Some("\"v2\""), SAMPLE_A)]);
    let fetch = catalog::fetch_cached_with(&http, dir.path()).unwrap();
    assert_eq!(fetch.source, FetchSource::Fresh);
    assert_eq!(http.requests.borrow()[0].1, None);
}

#[test]
fn corrupt_cache_meta_is_treated_as_absent() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("catalog-body.json"), SAMPLE_A).unwrap();
    std::fs::write(dir.path().join("catalog-meta.json"), "not json at all").unwrap();

    let http = FakeHttp::new(vec![ok(200, Some("\"v2\""), SAMPLE_B)]);
    let fetch = catalog::fetch_cached_with(&http, dir.path()).unwrap();
    assert_eq!(fetch.source, FetchSource::Fresh);
    assert_eq!(http.requests.borrow()[0].1, None);
}

#[test]
fn a_304_with_no_cache_at_all_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let http = FakeHttp::new(vec![ok(304, None, "")]);
    let err = catalog::fetch_cached_with(&http, dir.path());
    assert!(err.is_err());
}

// --- Icon cache -------------------------------------------------------------

#[test]
fn icon_downloads_once_then_reuses_the_cached_file() {
    let dir = tempfile::tempdir().unwrap();
    let url = "https://example.com/icons/foo-v1.png";
    let http = FakeHttp::new(vec![Ok(HttpResponse {
        status: 200,
        etag: None,
        body: vec![1, 2, 3, 4],
    })]);

    let path = catalog::cached_icon_with(&http, dir.path(), url).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), vec![1, 2, 3, 4]);
    assert_eq!(http.requests.borrow().len(), 1);

    // Second call for the same URL must not touch HTTP at all (the fake has no
    // more scripted responses, so an extra call would panic).
    let path2 = catalog::cached_icon_with(&http, dir.path(), url).unwrap();
    assert_eq!(path, path2);
    assert_eq!(http.requests.borrow().len(), 1);
}

#[test]
fn icon_filename_keeps_a_plain_image_extension() {
    let dir = tempfile::tempdir().unwrap();
    let cases = [
        ("https://example.com/a/icon.png", Some("png")),
        ("https://example.com/a/icon.PNG", Some("png")),
        ("https://example.com/a/icon.jpeg?x=1", Some("jpeg")),
        ("https://example.com/a/icon.webp#frag", Some("webp")),
        ("https://example.com/a/icon.svg", None),
        ("https://example.com/a/no-extension", None),
    ];
    for (url, expected_ext) in cases {
        let http = FakeHttp::new(vec![Ok(HttpResponse {
            status: 200,
            etag: None,
            body: vec![9],
        })]);
        let path = catalog::cached_icon_with(&http, dir.path(), url).unwrap();
        match expected_ext {
            Some(ext) => assert_eq!(path.extension().unwrap(), ext, "{url}"),
            None => assert_eq!(path.extension(), None, "{url}"),
        }
    }
}

#[test]
fn icon_cache_key_is_the_sha256_of_the_url() {
    use sha2::{Digest, Sha256};
    let dir = tempfile::tempdir().unwrap();
    let url = "https://example.com/icon.png";
    let http = FakeHttp::new(vec![Ok(HttpResponse {
        status: 200,
        etag: None,
        body: vec![9],
    })]);
    let path = catalog::cached_icon_with(&http, dir.path(), url).unwrap();
    let expected_hash = Sha256::digest(url.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(
        path.file_name().unwrap().to_str().unwrap(),
        format!("{expected_hash}.png")
    );
}

#[test]
fn icon_over_the_size_cap_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let too_big = vec![0u8; 6 * 1024 * 1024]; // > 5 MB cap
    let http = FakeHttp::new(vec![Ok(HttpResponse {
        status: 200,
        etag: None,
        body: too_big,
    })]);
    let err = catalog::cached_icon_with(&http, dir.path(), "https://example.com/huge.png");
    assert!(err.is_err());
    // Nothing partial left behind.
    let icons_dir = dir.path().join("icons");
    if icons_dir.exists() {
        assert_eq!(std::fs::read_dir(&icons_dir).unwrap().count(), 0);
    }
}

#[test]
fn non_https_icon_url_is_rejected_without_any_request() {
    let dir = tempfile::tempdir().unwrap();
    let http = FakeHttp::new(vec![]); // any call would panic
    let err = catalog::cached_icon_with(&http, dir.path(), "http://example.com/icon.png");
    assert!(err.is_err());
}

#[test]
fn prune_icons_removes_only_unreferenced_icons() {
    let dir = tempfile::tempdir().unwrap();
    let keep_url = "https://example.com/keep.png";
    let drop_url = "https://example.com/drop.png";

    let http = FakeHttp::new(vec![
        Ok(HttpResponse {
            status: 200,
            etag: None,
            body: vec![1],
        }),
        Ok(HttpResponse {
            status: 200,
            etag: None,
            body: vec![2],
        }),
    ]);
    let keep_path = catalog::cached_icon_with(&http, dir.path(), keep_url).unwrap();
    let drop_path = catalog::cached_icon_with(&http, dir.path(), drop_url).unwrap();
    assert!(keep_path.exists() && drop_path.exists());

    catalog::prune_icons(dir.path(), &[keep_url]).unwrap();
    assert!(keep_path.exists(), "referenced icon must survive pruning");
    assert!(!drop_path.exists(), "unreferenced icon must be deleted");
}

#[test]
fn prune_icons_on_a_missing_icons_dir_is_a_noop() {
    let dir = tempfile::tempdir().unwrap();
    catalog::prune_icons(dir.path(), &["https://example.com/x.png"]).unwrap();
}

#[test]
#[ignore = "hits the network"]
fn live_fetch_cached_twice_gets_not_modified() {
    let dir = tempfile::tempdir().unwrap();
    let first = catalog::fetch_cached(dir.path()).unwrap();
    assert_eq!(first.source, FetchSource::Fresh);
    assert!(first.entries.len() > 100);

    let second = catalog::fetch_cached(dir.path()).unwrap();
    assert_eq!(second.source, FetchSource::NotModified);
    assert_eq!(second.entries, first.entries);
}
