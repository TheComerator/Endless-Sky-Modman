//! Dependency and conflict checks over in-memory plugin states (decision C). No I/O.
//!
//! The game parses `requires`, `conflicts` and `game version` but enforces none of them, so
//! these checks are the only enforcement. Dependencies name plugin identities (`plugin.txt`
//! `name`, else folder name), which often differ from catalog names.

use std::cmp::Ordering;
use std::collections::HashSet;

use crate::catalog::CatalogEntry;
use crate::plugin_meta::PluginMeta;
use crate::records::InstallRecords;

// ---------------------------------------------------------------------------
// Identity matching
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogMatch<'a> {
    /// An install record maps the identity to this entry, or its catalog name is the identity.
    Exact(&'a CatalogEntry),
    /// The only entry whose normalized name equals the normalized identity. Must be confirmed
    /// against the downloaded `plugin.txt` (see [`Issue::IdentityMismatch`]).
    Likely(&'a CatalogEntry),
    /// More than one entry normalizes to the same name. Never resolved automatically.
    Ambiguous(Vec<&'a CatalogEntry>),
    NoMatch,
}

/// Lowercase, alphanumeric-only comparison key. Never fuzzy/edit-distance matched.
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// Resolves a plugin identity (or a catalog name) to a catalog entry.
///
/// Dependencies name identities, not catalog names, and the two often differ
/// (`Jimmys-Ship-Emporium` vs `Jimmy's Ship Emporium`), so this tries, in order:
/// an install record's known identity -> catalog-name mapping, an exact catalog
/// name match, then a normalized match (unique = Likely, several = Ambiguous).
pub fn match_catalog<'a>(
    identity_or_name: &str,
    catalog: &'a [CatalogEntry],
    records: &InstallRecords,
) -> CatalogMatch<'a> {
    if let Some(record) = records.values().find(|r| r.identity == identity_or_name)
        && let Some(entry) = catalog.iter().find(|e| e.name == record.catalog_name)
    {
        return CatalogMatch::Exact(entry);
    }
    if let Some(entry) = catalog.iter().find(|e| e.name == identity_or_name) {
        return CatalogMatch::Exact(entry);
    }
    let needle = normalize(identity_or_name);
    let hits: Vec<&CatalogEntry> = catalog
        .iter()
        .filter(|e| normalize(&e.name) == needle)
        .collect();
    match hits.len() {
        0 => CatalogMatch::NoMatch,
        1 => CatalogMatch::Likely(hits[0]),
        _ => CatalogMatch::Ambiguous(hits),
    }
}

// ---------------------------------------------------------------------------
// Game version comparison
//
// The engine's own `GameVersion` class (source/GameVersion.{h,cpp}) only ever
// formats a version (`major.minor.release.patch`, `-alpha` suffixed for a non-full
// release); it has no parser or comparison operators; nothing in the engine reads
// a version back in. `Plugin::PluginDependencies::gameVersion` (source/Plugin.h) is
// stored and shown as text in the plugin's description and never compared either.
// The wiki (CreatingPlugins) only says `"game version"` is "the game version(s)
// that this plugin is expected to function with" -- genuinely undocumented as to
// minimum/exact/range. We treat it as a MINIMUM required version (a plugin built
// against 0.10.13.1 is assumed to keep working on later releases), which is the
// only reading that lets "update your game" ever resolve the problem.
// ---------------------------------------------------------------------------

/// Dotted numeric components plus whether the string carries engine's `-alpha` suffix.
/// `None` for anything that doesn't parse as that shape (never guessed at).
fn parse_version(s: &str) -> Option<(Vec<u64>, bool)> {
    let (numeric, alpha) = match s.strip_suffix("-alpha") {
        Some(rest) => (rest, true),
        None => (s, false),
    };
    if numeric.is_empty() {
        return None;
    }
    let parts: Option<Vec<u64>> = numeric.split('.').map(|p| p.parse().ok()).collect();
    parts.map(|p| (p, alpha))
}

/// Compares two version strings the way the engine's own numbers would sort if it
/// compared them: dotted components, differing lengths padded with 0 (so `0.10.0` <
/// `0.10.13.1`), and a `-alpha` build ranks just below the same numbers without it.
/// `None` if either string doesn't parse.
pub fn compare_game_versions(a: &str, b: &str) -> Option<Ordering> {
    let (a_parts, a_alpha) = parse_version(a)?;
    let (b_parts, b_alpha) = parse_version(b)?;
    let len = a_parts.len().max(b_parts.len());
    for i in 0..len {
        let av = a_parts.get(i).copied().unwrap_or(0);
        let bv = b_parts.get(i).copied().unwrap_or(0);
        match av.cmp(&bv) {
            Ordering::Equal => continue,
            other => return Some(other),
        }
    }
    // Same numbers: a full release outranks its own `-alpha` build.
    Some(b_alpha.cmp(&a_alpha))
}

/// `None` (satisfied) unless the actual version is unknown/unparseable (`GameVersionUnknown`)
/// or parses lower than `required` (`GameTooOld`).
fn game_version_issue(plugin: &str, required: &str, actual: Option<&str>) -> Option<Issue> {
    match actual.and_then(|a| compare_game_versions(a, required).map(|ord| (a, ord))) {
        Some((actual, Ordering::Less)) => Some(Issue::GameTooOld {
            plugin: plugin.to_string(),
            required: required.to_string(),
            actual: actual.to_string(),
        }),
        Some(_) => None,
        None => Some(Issue::GameVersionUnknown {
            plugin: plugin.to_string(),
            required: required.to_string(),
        }),
    }
}

// ---------------------------------------------------------------------------
// Issues and notes
// ---------------------------------------------------------------------------

/// Everything that blocks an operation (decision C: every problem warns AND blocks,
/// with an explicit user override to proceed anyway).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Issue {
    /// `requires`'d identity isn't in the final state and no catalog entry matches it.
    MissingRequirement {
        plugin: String,
        requires: String,
    },
    /// `requires`'d identity isn't in the final state and several catalog entries normalize
    /// to it.
    AmbiguousRequirement {
        plugin: String,
        requires: String,
        candidates: Vec<String>,
    },
    /// A `Likely` catalog match was downloaded, but its `plugin.txt` identity differs from
    /// what was required.
    IdentityMismatch {
        expected: String,
        catalog_name: String,
        actual: String,
    },
    /// Two plugins that would both end up enabled, where either declares the other in
    /// `conflicts` (the declaration is one-sided; both directions are checked).
    Conflict {
        a: String,
        b: String,
    },
    GameTooOld {
        plugin: String,
        required: String,
        actual: String,
    },
    GameVersionUnknown {
        plugin: String,
        required: String,
    },
    /// Disabling/uninstalling `plugin` would leave `dependents` (enabled, requiring it)
    /// unsatisfied.
    RequiredBy {
        plugin: String,
        dependents: Vec<String>,
    },
    CatalogDownloadFailed {
        catalog_name: String,
        error: String,
    },
}

/// Non-blocking information, never forced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    OptionalAvailable {
        plugin: String,
        optional: String,
        installed: bool,
    },
}

// ---------------------------------------------------------------------------
// Checks over a description of the final state
// ---------------------------------------------------------------------------

/// A plugin as it would exist after an operation commits: installed, with its
/// `plugin.txt` metadata, and either enabled or disabled. The same shape serves
/// install, enable, disable, uninstall, update and profile-apply: each just builds
/// a different `final_state` and runs the same checks over it.
#[derive(Debug, Clone, Copy)]
pub struct PlannedPlugin<'a> {
    pub identity: &'a str,
    pub enabled: bool,
    pub meta: &'a PluginMeta,
}

/// Conflicts among plugins that would end up enabled together. No recursion, so
/// cycles in `conflicts` (there's no reason for one, but nothing rules it out)
/// can't loop.
pub fn check_conflicts(final_state: &[PlannedPlugin]) -> Vec<Issue> {
    let enabled: Vec<&PlannedPlugin> = final_state.iter().filter(|p| p.enabled).collect();
    let mut issues = Vec::new();
    for i in 0..enabled.len() {
        for j in (i + 1)..enabled.len() {
            let (a, b) = (enabled[i], enabled[j]);
            if a.meta.dependencies.conflicts.contains(b.identity)
                || b.meta.dependencies.conflicts.contains(a.identity)
            {
                issues.push(Issue::Conflict {
                    a: a.identity.to_string(),
                    b: b.identity.to_string(),
                });
            }
        }
    }
    issues
}

/// Game-version problems for every enabled plugin that declares one. `game_version` is
/// the installed game's own version, when known.
pub fn check_game_version(final_state: &[PlannedPlugin], game_version: Option<&str>) -> Vec<Issue> {
    final_state
        .iter()
        .filter(|p| p.enabled)
        .filter_map(|p| {
            let required = p.meta.dependencies.game_version.as_deref()?;
            game_version_issue(p.identity, required, game_version)
        })
        .collect()
}

/// Conflicts plus game-version problems: the two checks that only need the final
/// state itself, no catalog or install records. See [`check_requirements`] and
/// [`required_by`] for the checks that do.
pub fn check_state(final_state: &[PlannedPlugin], game_version: Option<&str>) -> Vec<Issue> {
    let mut issues = check_conflicts(final_state);
    issues.extend(check_game_version(final_state, game_version));
    issues
}

/// `requires` edges not satisfied within `final_state`. A catalog match (`Likely`/`Exact`)
/// still blocks here -- it isn't actually in the plan -- but tells the caller the problem is
/// fixable by installing it first (see `match_catalog`).
pub fn check_requirements(
    final_state: &[PlannedPlugin],
    catalog: &[CatalogEntry],
    records: &InstallRecords,
) -> Vec<Issue> {
    let present: HashSet<&str> = final_state
        .iter()
        .filter(|p| p.enabled)
        .map(|p| p.identity)
        .collect();
    let mut issues = Vec::new();
    for p in final_state.iter().filter(|p| p.enabled) {
        for requires in &p.meta.dependencies.requires {
            if present.contains(requires.as_str()) {
                continue;
            }
            match match_catalog(requires, catalog, records) {
                CatalogMatch::Ambiguous(candidates) => issues.push(Issue::AmbiguousRequirement {
                    plugin: p.identity.to_string(),
                    requires: requires.clone(),
                    candidates: candidates.into_iter().map(|e| e.name.clone()).collect(),
                }),
                CatalogMatch::NoMatch | CatalogMatch::Exact(_) | CatalogMatch::Likely(_) => {
                    issues.push(Issue::MissingRequirement {
                        plugin: p.identity.to_string(),
                        requires: requires.clone(),
                    });
                }
            }
        }
    }
    issues
}

/// Enabled plugins in `final_state` that `requires` the given identity. An empty result
/// means disabling/uninstalling it is safe.
pub fn required_by(identity: &str, final_state: &[PlannedPlugin]) -> Vec<String> {
    final_state
        .iter()
        .filter(|p| {
            p.enabled && p.identity != identity && p.meta.dependencies.requires.contains(identity)
        })
        .map(|p| p.identity.to_string())
        .collect()
}

/// Non-blocking notes: optional dependencies of every enabled plugin, and whether each is
/// currently in the plan (installed or staged), enabled or not.
pub fn optional_notes(final_state: &[PlannedPlugin]) -> Vec<Note> {
    let present: HashSet<&str> = final_state.iter().map(|p| p.identity).collect();
    final_state
        .iter()
        .filter(|p| p.enabled)
        .flat_map(|p| {
            p.meta
                .dependencies
                .optional
                .iter()
                .map(|optional| Note::OptionalAvailable {
                    plugin: p.identity.to_string(),
                    optional: optional.clone(),
                    installed: present.contains(optional.as_str()),
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_meta::Dependencies;
    use crate::records::InstallRecord;

    fn entry(name: &str) -> CatalogEntry {
        CatalogEntry {
            name: name.to_string(),
            authors: "A".into(),
            homepage: "https://example.com".into(),
            license: "MIT".into(),
            version: "1.0".into(),
            short_description: "".into(),
            description: None,
            url: "https://example.com/a.zip".into(),
            icon_url: None,
            autoupdate: None,
        }
    }

    fn record(identity: &str, catalog_name: &str) -> InstallRecord {
        InstallRecord {
            catalog_name: catalog_name.into(),
            folder: catalog_name.into(),
            identity: identity.into(),
            version: "1.0".into(),
            source_url: "https://example.com/a.zip".into(),
            sha256: "ab".repeat(32),
        }
    }

    fn meta(name: Option<&str>, deps: Dependencies) -> PluginMeta {
        PluginMeta {
            name: name.map(str::to_string),
            dependencies: deps,
            ..PluginMeta::default()
        }
    }

    fn requires(names: &[&str]) -> Dependencies {
        Dependencies {
            requires: names.iter().map(|s| s.to_string()).collect(),
            ..Dependencies::default()
        }
    }

    fn conflicts(names: &[&str]) -> Dependencies {
        Dependencies {
            conflicts: names.iter().map(|s| s.to_string()).collect(),
            ..Dependencies::default()
        }
    }

    // --- identity matching ---

    #[test]
    fn exact_match_by_name_and_by_record() {
        let catalog = [entry("Jimmys-Ship-Emporium"), entry("Other")];
        let records = InstallRecords::new();
        assert_eq!(
            match_catalog("Jimmys-Ship-Emporium", &catalog, &records),
            CatalogMatch::Exact(&catalog[0])
        );

        let mut records = InstallRecords::new();
        records.insert(
            "f".into(),
            record("Jimmy's Ship Emporium", "Jimmys-Ship-Emporium"),
        );
        assert_eq!(
            match_catalog("Jimmy's Ship Emporium", &catalog, &records),
            CatalogMatch::Exact(&catalog[0])
        );
    }

    #[test]
    fn likely_match_is_normalized_equality() {
        let catalog = [entry("Jimmys-Ship-Emporium")];
        let records = InstallRecords::new();
        assert_eq!(
            match_catalog("Jimmy's Ship Emporium", &catalog, &records),
            CatalogMatch::Likely(&catalog[0])
        );
    }

    #[test]
    fn ambiguous_normalized_match() {
        let catalog = [entry("Mega-Freight"), entry("Mega Freight!")];
        let records = InstallRecords::new();
        match match_catalog("Mega Freight", &catalog, &records) {
            CatalogMatch::Ambiguous(hits) => assert_eq!(hits.len(), 2),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn no_match() {
        let catalog = [entry("Something Else")];
        let records = InstallRecords::new();
        assert_eq!(
            match_catalog("Nope", &catalog, &records),
            CatalogMatch::NoMatch
        );
    }

    #[test]
    fn never_fuzzy_matches() {
        // One character off after normalizing ("emporia" vs "emporium") must not match.
        let catalog = [entry("Jimmys Ship Emporia")];
        let records = InstallRecords::new();
        assert_eq!(
            match_catalog("Jimmys Ship Emporium", &catalog, &records),
            CatalogMatch::NoMatch
        );
    }

    // --- game version ---

    #[test]
    fn version_comparison_handles_alpha_and_uneven_lengths() {
        assert_eq!(
            compare_game_versions("0.10.0", "0.10.13.1"),
            Some(Ordering::Less)
        );
        assert_eq!(
            compare_game_versions("0.11.3.0", "0.11.3.0"),
            Some(Ordering::Equal)
        );
        assert_eq!(
            compare_game_versions("0.11.4.0-alpha", "0.11.4.0"),
            Some(Ordering::Less),
            "an alpha build ranks below the same numbers without the suffix"
        );
        assert_eq!(compare_game_versions("not a version", "0.10.0"), None);
    }

    #[test]
    fn game_too_old_unknown_and_satisfied() {
        let too_old = meta(
            Some("P"),
            Dependencies {
                game_version: Some("0.10.13.1".into()),
                ..Dependencies::default()
            },
        );
        let planned = [PlannedPlugin {
            identity: "P",
            enabled: true,
            meta: &too_old,
        }];
        assert_eq!(
            check_game_version(&planned, Some("0.10.0")),
            [Issue::GameTooOld {
                plugin: "P".into(),
                required: "0.10.13.1".into(),
                actual: "0.10.0".into(),
            }]
        );
        assert_eq!(
            check_game_version(&planned, None),
            [Issue::GameVersionUnknown {
                plugin: "P".into(),
                required: "0.10.13.1".into(),
            }]
        );
        assert_eq!(check_game_version(&planned, Some("0.11.3.0")), []);

        let no_requirement = meta(Some("Q"), Dependencies::default());
        let planned = [PlannedPlugin {
            identity: "Q",
            enabled: true,
            meta: &no_requirement,
        }];
        assert_eq!(check_game_version(&planned, None), []);
    }

    // --- conflicts ---

    #[test]
    fn conflict_detected_in_either_declaration_direction() {
        let a = meta(Some("A"), conflicts(&["B"]));
        let b = meta(Some("B"), Dependencies::default());
        let planned = [
            PlannedPlugin {
                identity: "A",
                enabled: true,
                meta: &a,
            },
            PlannedPlugin {
                identity: "B",
                enabled: true,
                meta: &b,
            },
        ];
        assert_eq!(
            check_conflicts(&planned),
            [Issue::Conflict {
                a: "A".into(),
                b: "B".into()
            }]
        );

        // Same pair, declared only by the other side.
        let a2 = meta(Some("A"), Dependencies::default());
        let b2 = meta(Some("B"), conflicts(&["A"]));
        let planned2 = [
            PlannedPlugin {
                identity: "A",
                enabled: true,
                meta: &a2,
            },
            PlannedPlugin {
                identity: "B",
                enabled: true,
                meta: &b2,
            },
        ];
        assert_eq!(
            check_conflicts(&planned2),
            [Issue::Conflict {
                a: "A".into(),
                b: "B".into()
            }]
        );
    }

    #[test]
    fn disabled_plugins_cannot_conflict() {
        let a = meta(Some("A"), conflicts(&["B"]));
        let b = meta(Some("B"), Dependencies::default());
        let planned = [
            PlannedPlugin {
                identity: "A",
                enabled: true,
                meta: &a,
            },
            PlannedPlugin {
                identity: "B",
                enabled: false,
                meta: &b,
            },
        ];
        assert_eq!(check_conflicts(&planned), []);
    }

    // --- requirements ---

    #[test]
    fn requirement_satisfied_missing_and_ambiguous() {
        // BTreeSet order: "B" < "Dup" < "Missing".
        let a = meta(Some("A"), requires(&["B", "Dup", "Missing"]));
        let b = meta(Some("B"), Dependencies::default());
        let planned = [
            PlannedPlugin {
                identity: "A",
                enabled: true,
                meta: &a,
            },
            PlannedPlugin {
                identity: "B",
                enabled: true,
                meta: &b,
            },
        ];
        // Both normalize to "dup" without equaling "Dup" exactly, so neither is an Exact match.
        let catalog = [entry("Dup!"), entry("DUP")];
        let records = InstallRecords::new();
        let issues = check_requirements(&planned, &catalog, &records);
        assert_eq!(
            issues,
            [
                Issue::AmbiguousRequirement {
                    plugin: "A".into(),
                    requires: "Dup".into(),
                    candidates: vec!["Dup!".into(), "DUP".into()],
                },
                Issue::MissingRequirement {
                    plugin: "A".into(),
                    requires: "Missing".into()
                },
            ]
        );
    }

    #[test]
    fn requirement_installed_but_disabled_is_not_satisfied() {
        let a = meta(Some("A"), requires(&["B"]));
        let b = meta(Some("B"), Dependencies::default());
        let planned = [
            PlannedPlugin {
                identity: "A",
                enabled: true,
                meta: &a,
            },
            PlannedPlugin {
                identity: "B",
                enabled: false,
                meta: &b,
            },
        ];
        assert_eq!(
            check_requirements(&planned, &[], &InstallRecords::new()),
            [Issue::MissingRequirement {
                plugin: "A".into(),
                requires: "B".into()
            }]
        );
    }

    // --- required_by ---

    #[test]
    fn required_by_lists_enabled_dependents_only() {
        let a = meta(Some("A"), requires(&["B"]));
        let c = meta(Some("C"), requires(&["B"]));
        let other = meta(Some("Other"), Dependencies::default());
        let planned = [
            PlannedPlugin {
                identity: "A",
                enabled: true,
                meta: &a,
            },
            PlannedPlugin {
                identity: "C",
                enabled: false,
                meta: &c,
            },
            PlannedPlugin {
                identity: "Other",
                enabled: true,
                meta: &other,
            },
        ];
        assert_eq!(required_by("B", &planned), ["A".to_string()]);
        assert_eq!(required_by("Other", &planned), Vec::<String>::new());
    }

    // --- optional notes ---

    #[test]
    fn optional_notes_report_presence() {
        let a = meta(
            Some("A"),
            Dependencies {
                optional: ["B".to_string(), "Absent".to_string()].into(),
                ..Dependencies::default()
            },
        );
        let b = meta(Some("B"), Dependencies::default());
        let planned = [
            PlannedPlugin {
                identity: "A",
                enabled: true,
                meta: &a,
            },
            PlannedPlugin {
                identity: "B",
                enabled: false,
                meta: &b,
            },
        ];
        let mut notes = optional_notes(&planned);
        notes.sort_by(|n1, n2| {
            let Note::OptionalAvailable { optional: o1, .. } = n1;
            let Note::OptionalAvailable { optional: o2, .. } = n2;
            o1.cmp(o2)
        });
        assert_eq!(
            notes,
            [
                Note::OptionalAvailable {
                    plugin: "A".into(),
                    optional: "Absent".into(),
                    installed: false,
                },
                Note::OptionalAvailable {
                    plugin: "A".into(),
                    optional: "B".into(),
                    installed: true,
                },
            ]
        );
    }
}
