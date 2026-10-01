//! Tests against files captured from real plugins and the live catalog (2026-10-01).

use esmm_core::{catalog, plugin_meta::PluginMeta};

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn plugin_txt(name: &str) -> PluginMeta {
    PluginMeta::parse(&fixture(&format!("plugin-txt/{name}")))
}

#[test]
fn catalog_snapshot_parses_every_entry() {
    let entries = catalog::parse(&fixture("catalog-2026-10-01.json")).unwrap();
    assert_eq!(entries.len(), 165);
    assert_eq!(
        entries.iter().filter(|e| e.icon_url.is_some()).count(),
        151,
        "iconUrl + the one iconURL"
    );
    assert_eq!(
        entries.iter().filter(|e| e.autoupdate.is_some()).count(),
        163
    );
    assert_eq!(
        entries.iter().filter(|e| e.description.is_none()).count(),
        1
    );
}

#[test]
fn mega_freight_dependencies() {
    let meta = plugin_txt("mega-freight.txt");
    assert_eq!(meta.name.as_deref(), Some("Mega Freight"));
    assert_eq!(meta.version.as_deref(), Some("1.0.0010101000100100111"));
    assert_eq!(meta.about.lines().count(), 2);
    let deps = &meta.dependencies;
    assert_eq!(deps.game_version.as_deref(), Some("0.10.13.1"));
    assert!(deps.requires.is_empty());
    assert!(deps.optional.contains("Mega Freight: Weapon Pack"));
    assert!(deps.optional.contains("Zoura's Outfits Expanded"));
    assert!(deps.conflicts.contains("Mini Freight"));
    assert!(meta.tags.contains("weapons"));
}

#[test]
fn backtick_quoted_name() {
    let meta = plugin_txt("disable-aberrant-blockade.txt");
    assert_eq!(meta.name.as_deref(), Some("Disable Aberrant Blockade"));
    assert_eq!(meta.version.as_deref(), Some("1.0.0"));
}

#[test]
fn catalog_and_plugin_txt_names_can_differ() {
    let meta = plugin_txt("jimmys-ship-emporium.txt");
    assert_eq!(meta.name.as_deref(), Some("Jimmy's Ship Emporium"));
    assert_eq!(
        meta.identity("Jimmys-Ship-Emporium"),
        "Jimmy's Ship Emporium"
    );
}

#[test]
fn bare_tokens_and_repeated_about() {
    let meta = plugin_txt("bunsen-burner.txt");
    assert_eq!(meta.name.as_deref(), Some("Bunsen.Burner"));
    assert_eq!(meta.about.lines().count(), 2);
    assert_eq!(meta.dependencies.game_version.as_deref(), Some("0.10.8"));
}

#[test]
fn authors_list() {
    let meta = plugin_txt("ship-merging.txt");
    assert_eq!(meta.authors.len(), 2);
    assert!(meta.authors.contains("unknown_rawrs"));
}

#[test]
fn folder_name_is_identity_without_plugin_txt_name() {
    let meta = PluginMeta::parse("");
    assert_eq!(meta.identity("A-Coalition-At-War"), "A-Coalition-At-War");
}

#[test]
#[ignore = "hits the network; run with --ignored"]
fn live_catalog_fetch() {
    let entries = catalog::fetch().unwrap();
    assert!(entries.len() > 100, "got {}", entries.len());
}
