//! Plugin overlap detection: which plugins touch the same game content.
//!
//! The game loads plugin folders in sorted (alphabetical) order, reads each one's `data`
//! files in that order, and a later full definition of an object (say `ship "Hornet"`)
//! replaces an earlier one. Lines that only `add` to or `remove` from an object layer on top
//! of whatever is there. The manager can't see inside the game, but it can read each plugin's
//! files and warn *before* a plugin is activated that it would replace, or be replaced by,
//! another one. Plugin data is compared with other plugins only, never with the base game
//! (plugins are assumed to override vanilla files).
//!
//! Each plugin is read once into a small [`PluginIndex`] (what it defines, plus its image and
//! sound paths) that is cached on disk with a fingerprint, so only plugins that changed are
//! read again. Comparing indexes is then cheap, and works for disabled plugins too.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::datanode::{self, Node};
use crate::files;

/// Bump when the index layout or its classification rules change, so stale caches are rebuilt.
pub const INDEX_FORMAT: u32 = 1;

/// How a plugin's top-level definition treats an object that may already exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Only `add` / `remove` lines: layers onto whatever is there. Not a conflict.
    Extends,
    /// A full definition (or one marked `overwrite`): replaces any earlier one.
    Replaces,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Definition {
    /// The node type: `ship`, `outfit`, `system`, ...
    pub kind: String,
    /// The object's name. For a ship variant (`ship "Falcon" "Falcon Mk2"`) both names, joined.
    pub name: String,
    pub mode: Mode,
}

/// `Default` (format 0, empty) is what a missing cache file loads as; it never matches the real
/// format, so it counts as a cache miss.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginIndex {
    pub format: u32,
    /// Hash of the plugin's file list and sizes: if it matches, the index is still valid.
    pub fingerprint: String,
    pub definitions: BTreeSet<Definition>,
    /// Image names (no extension or frame number). High-resolution `@2x` art keeps its suffix,
    /// so a plugin supplying sharper copies of another's images isn't flagged against it.
    pub images: BTreeSet<String>,
    pub sounds: BTreeSet<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum OverlapError {
    #[error("failed to read the plugin: {0}")]
    Io(#[from] io::Error),
    #[error("overlap cache is corrupt: {0}")]
    Json(#[from] serde_json::Error),
}

// ---------------------------------------------------------------------------
// Reading one plugin
// ---------------------------------------------------------------------------

/// Node types that are bookkeeping, not objects that can be replaced.
const IGNORED_KINDS: &[&str] = &["disable", "overwrite"];

/// Whether a top-level node only layers changes onto an object (`add` / `remove` lines).
fn classify(node: &Node, marked_overwrite: bool) -> Mode {
    if marked_overwrite {
        return Mode::Replaces;
    }
    let only_layering = !node.children.is_empty()
        && node
            .children
            .iter()
            .all(|c| matches!(c.key(), "add" | "remove"));
    if only_layering {
        Mode::Extends
    } else {
        Mode::Replaces
    }
}

/// The definitions in one data file's text.
pub fn definitions_in(text: &str) -> BTreeSet<Definition> {
    let mut found = BTreeSet::new();
    let mut overwrite_next = false;
    for node in datanode::parse(text) {
        let kind = node.key();
        if kind == "overwrite" {
            // A marker on its own line: the next node replaces whatever was there.
            overwrite_next = true;
            continue;
        }
        let marked = std::mem::take(&mut overwrite_next);
        if IGNORED_KINDS.contains(&kind) || node.tokens.len() < 2 {
            continue;
        }
        let name = node.tokens[1..node.tokens.len().min(3)].join(" / ");
        found.insert(Definition {
            kind: kind.to_string(),
            name,
            mode: classify(&node, marked),
        });
    }
    found
}

/// An image or sound path as the game names it: no extension, no frame number.
/// `ship/hornet-0@2x.png` -> `ship/hornet@2x`; `land/aera.jpg` -> `land/aera`.
pub fn asset_name(relative: &str, strip_frames: bool) -> String {
    let path = relative.replace('\\', "/");
    let without_ext = match path.rfind('.') {
        Some(dot) if !path[dot..].contains('/') => &path[..dot],
        _ => path.as_str(),
    };
    // `@2x`, `@3x`: high-resolution art, kept apart from the normal-size art.
    let (stem, scale) = match without_ext.rfind('@') {
        Some(at)
            if without_ext[at + 1..].ends_with('x')
                && without_ext[at + 1..without_ext.len() - 1]
                    .chars()
                    .all(|c| c.is_ascii_digit()) =>
        {
            (&without_ext[..at], &without_ext[at..])
        }
        _ => (without_ext, ""),
    };
    let stem = if strip_frames {
        strip_frame(stem)
    } else {
        stem
    };
    format!("{stem}{scale}")
}

/// Removes a trailing frame marker such as `-12`, `~3`, `+0` or `=1`.
fn strip_frame(stem: &str) -> &str {
    let digits = stem.chars().rev().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits == stem.len() {
        return stem;
    }
    let before = &stem[..stem.len() - digits];
    match before.chars().last() {
        Some('-' | '~' | '+' | '=') => &before[..before.len() - 1],
        _ => stem,
    }
}

struct FileEntry {
    relative: String,
    size: u64,
}

/// Every regular file under `root/sub` (symlinks aren't followed), sorted by relative path.
fn walk(root: &Path, sub: &str) -> io::Result<Vec<FileEntry>> {
    let base = root.join(sub);
    let mut out = Vec::new();
    let mut pending = vec![base.clone()];
    while let Some(dir) = pending.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            // A plugin needn't have every folder.
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        for entry in entries {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                let relative = entry
                    .path()
                    .strip_prefix(&base)
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                out.push(FileEntry {
                    relative,
                    size: entry.metadata()?.len(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(out)
}

fn fingerprint_of(groups: &[(&str, &[FileEntry])]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(INDEX_FORMAT.to_le_bytes());
    for (name, files) in groups {
        hasher.update(name.as_bytes());
        for f in *files {
            hasher.update(f.relative.as_bytes());
            hasher.update([0]);
            hasher.update(f.size.to_le_bytes());
        }
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Reads one plugin folder into an index. Read-only: nothing in the folder is changed.
pub fn index_plugin(plugin_dir: &Path) -> Result<PluginIndex, OverlapError> {
    let data = walk(plugin_dir, "data")?;
    let images = walk(plugin_dir, "images")?;
    let sounds = walk(plugin_dir, "sounds")?;
    let fingerprint = fingerprint_of(&[("data", &data), ("images", &images), ("sounds", &sounds)]);

    // Every data file is read in full, however large: skipping one would hide real overlaps.
    let mut definitions = BTreeSet::new();
    for file in data
        .iter()
        .filter(|f| f.relative.to_ascii_lowercase().ends_with(".txt"))
    {
        let bytes = fs::read(plugin_dir.join("data").join(&file.relative))?;
        definitions.extend(definitions_in(&String::from_utf8_lossy(&bytes)));
    }

    let is_image = |p: &str| {
        let p = p.to_ascii_lowercase();
        [".png", ".jpg", ".jpeg"].iter().any(|e| p.ends_with(e))
    };
    let is_sound = |p: &str| {
        let p = p.to_ascii_lowercase();
        [".wav", ".mp3", ".flac", ".ogg"]
            .iter()
            .any(|e| p.ends_with(e))
    };
    Ok(PluginIndex {
        format: INDEX_FORMAT,
        fingerprint,
        definitions,
        images: images
            .iter()
            .filter(|f| is_image(&f.relative))
            .map(|f| asset_name(&f.relative, true))
            .collect(),
        sounds: sounds
            .iter()
            .filter(|f| is_sound(&f.relative))
            .map(|f| asset_name(&f.relative, false))
            .collect(),
    })
}

/// The cached index at `cache_file` if it still matches the plugin's files, otherwise a fresh
/// one, which is saved to the cache. A missing or corrupt cache is just a cache miss. The
/// plugin folder itself is only ever read.
pub fn load_or_index(plugin_dir: &Path, cache_file: &Path) -> Result<PluginIndex, OverlapError> {
    let current = {
        let data = walk(plugin_dir, "data")?;
        let images = walk(plugin_dir, "images")?;
        let sounds = walk(plugin_dir, "sounds")?;
        fingerprint_of(&[("data", &data), ("images", &images), ("sounds", &sounds)])
    };
    if let Ok(cached) = files::load_json::<PluginIndex, OverlapError>(cache_file)
        && cached.format == INDEX_FORMAT
        && cached.fingerprint == current
    {
        return Ok(cached);
    }
    let fresh = index_plugin(plugin_dir)?;
    files::save_json::<_, OverlapError>(cache_file, &fresh)?;
    Ok(fresh)
}

// ---------------------------------------------------------------------------
// Comparing plugins
// ---------------------------------------------------------------------------

/// One plugin as the comparison sees it: its folder name (which sets the load order) and index.
pub struct PluginEntry<'a> {
    pub folder: &'a str,
    pub index: &'a PluginIndex,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum OverlapItem {
    Object { kind: String, name: String },
    Image(String),
    Sound(String),
}

/// Something one plugin replaces from others. `winner` loads last, so its version is the
/// one the game uses; `overridden` lists the earlier plugins whose version is lost.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Overlap {
    pub item: OverlapItem,
    pub winner: String,
    pub overridden: Vec<String>,
}

/// The overlaps among `plugins`, in the game's load order (sorted by folder name). Objects are
/// reported only when a full definition comes *after* another plugin's definition of the same
/// thing (what an earlier plugin did is lost). A plugin that only `add`s / `remove`s, or that
/// loads after the full definition, layers on top and is fine. Image and sound names shared by
/// two plugins are reported with the later one winning.
pub fn find_overlaps(plugins: &[PluginEntry]) -> Vec<Overlap> {
    let mut ordered: Vec<&PluginEntry> = plugins.iter().collect();
    ordered.sort_by(|a, b| a.folder.cmp(b.folder));

    let mut objects: BTreeMap<(&str, &str), Vec<(&str, Mode)>> = BTreeMap::new();
    let mut images: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut sounds: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for p in &ordered {
        for d in &p.index.definitions {
            objects
                .entry((d.kind.as_str(), d.name.as_str()))
                .or_default()
                .push((p.folder, d.mode));
        }
        for i in &p.index.images {
            images.entry(i.as_str()).or_default().push(p.folder);
        }
        for s in &p.index.sounds {
            sounds.entry(s.as_str()).or_default().push(p.folder);
        }
    }

    let mut found = Vec::new();
    for ((kind, name), defs) in objects {
        // The last full definition decides the object; anything before it from another
        // plugin has been replaced.
        let Some(last) = defs.iter().rposition(|(_, m)| *m == Mode::Replaces) else {
            continue;
        };
        let winner = defs[last].0;
        let mut overridden: Vec<String> = defs[..last]
            .iter()
            .map(|(f, _)| (*f).to_string())
            .filter(|f| f != winner)
            .collect();
        overridden.dedup();
        if !overridden.is_empty() {
            found.push(Overlap {
                item: OverlapItem::Object {
                    kind: kind.to_string(),
                    name: name.to_string(),
                },
                winner: winner.to_string(),
                overridden,
            });
        }
    }
    for (map, make) in [
        (
            images,
            (|s: &str| OverlapItem::Image(s.to_string())) as fn(&str) -> OverlapItem,
        ),
        (sounds, |s: &str| OverlapItem::Sound(s.to_string())),
    ] {
        for (name, owners) in map {
            if owners.len() > 1 {
                found.push(Overlap {
                    item: make(name),
                    winner: owners[owners.len() - 1].to_string(),
                    overridden: owners[..owners.len() - 1]
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                });
            }
        }
    }
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(kind: &str, name: &str, mode: Mode) -> Definition {
        Definition {
            kind: kind.into(),
            name: name.into(),
            mode,
        }
    }

    fn index(defs: &[Definition], images: &[&str]) -> PluginIndex {
        PluginIndex {
            format: INDEX_FORMAT,
            fingerprint: String::new(),
            definitions: defs.iter().cloned().collect(),
            images: images.iter().map(|s| s.to_string()).collect(),
            sounds: BTreeSet::new(),
        }
    }

    #[test]
    fn add_and_remove_only_definitions_extend_everything_else_replaces() {
        let text = "\
system \"1 Axis\"
\tremove minables lead
\tadd minables \"Pb pebble\" 1 3.87
system \"2 Axis\"
\tadd fleet \"space fauna 01\" 19552
\tpos 10 20
ship \"Hornet\"
\tsprite \"ship/hornet\"
\tattributes
\t\t\"shields\" 1900
ship \"Falcon\" \"Falcon Mk2\"
\tsprite \"ship/falcon\"
overwrite
outfit \"Thing\"
\tadd attributes
disable mission \"Old\"
start
";
        let defs = definitions_in(text);
        assert!(defs.contains(&def("system", "1 Axis", Mode::Extends)));
        assert!(
            defs.contains(&def("system", "2 Axis", Mode::Replaces)),
            "a real field alongside `add` is a full definition"
        );
        assert!(defs.contains(&def("ship", "Hornet", Mode::Replaces)));
        assert!(defs.contains(&def("ship", "Falcon / Falcon Mk2", Mode::Replaces)));
        assert!(
            defs.contains(&def("outfit", "Thing", Mode::Replaces)),
            "`overwrite` on the line before forces a replace"
        );
        assert!(
            !defs
                .iter()
                .any(|d| d.kind == "disable" || d.kind == "start"),
            "bookkeeping and nameless nodes are ignored"
        );
        assert_eq!(defs.len(), 5);
    }

    #[test]
    fn asset_names_drop_extensions_and_frames_but_keep_high_resolution_apart() {
        assert_eq!(asset_name("ship/hornet-0.png", true), "ship/hornet");
        assert_eq!(asset_name("ship/hornet-12@2x.png", true), "ship/hornet@2x");
        assert_eq!(asset_name("effect/spark~3.png", true), "effect/spark");
        assert_eq!(asset_name("land/aera.jpg", true), "land/aera");
        assert_eq!(asset_name("land\\aera@2x.jpg", true), "land/aera@2x");
        assert_eq!(
            asset_name("planet/2.png", true),
            "planet/2",
            "a bare number is a name"
        );
        assert_eq!(
            asset_name("planet/x-1.png", false),
            "planet/x-1",
            "sounds keep it"
        );
        assert_eq!(asset_name("sounds/hum~.wav", false), "sounds/hum~");
    }

    #[test]
    fn indexing_a_real_looking_plugin_folder_reads_data_images_and_sounds() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("data/sub")).unwrap();
        fs::create_dir_all(root.join("images/ship")).unwrap();
        fs::create_dir_all(root.join("sounds")).unwrap();
        fs::write(root.join("data/a.txt"), "ship \"A\"\n\tsprite \"ship/a\"\n").unwrap();
        fs::write(
            root.join("data/sub/b.TXT"),
            "system \"S\"\n\tadd fleet \"f\" 1\n",
        )
        .unwrap();
        fs::write(root.join("data/notes.md"), "ship \"Ignored\"\n\tx\n").unwrap();
        fs::write(root.join("images/ship/a-0.png"), b"x").unwrap();
        fs::write(root.join("images/ship/a-1.png"), b"x").unwrap();
        fs::write(root.join("images/ship/a-0@2x.png"), b"x").unwrap();
        fs::write(root.join("sounds/boom.wav"), b"x").unwrap();
        fs::write(root.join("sounds/readme.txt"), b"x").unwrap();

        let idx = index_plugin(root).unwrap();
        assert_eq!(
            idx.definitions.iter().cloned().collect::<Vec<_>>(),
            [
                def("ship", "A", Mode::Replaces),
                def("system", "S", Mode::Extends)
            ]
        );
        assert_eq!(
            idx.images.iter().cloned().collect::<Vec<_>>(),
            ["ship/a", "ship/a@2x"]
        );
        assert_eq!(idx.sounds.iter().cloned().collect::<Vec<_>>(), ["boom"]);
    }

    #[test]
    fn the_cache_is_reused_until_the_files_change() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = dir.path().join("plugin");
        fs::create_dir_all(plugin.join("data")).unwrap();
        fs::write(plugin.join("data/a.txt"), "ship \"A\"\n\tsprite x\n").unwrap();
        let cache = dir.path().join("cache/plugin.json");

        let first = load_or_index(&plugin, &cache).unwrap();
        assert!(cache.exists());

        // Poison the cache's content but keep its fingerprint: a hit must return it as is.
        let mut marked = first.clone();
        marked
            .definitions
            .insert(def("ship", "FromCache", Mode::Replaces));
        files::save_json::<_, OverlapError>(&cache, &marked).unwrap();
        assert_eq!(load_or_index(&plugin, &cache).unwrap(), marked);

        // A new file changes the fingerprint, so the plugin is read again.
        fs::write(plugin.join("data/b.txt"), "outfit \"B\"\n\tcost 1\n").unwrap();
        let again = load_or_index(&plugin, &cache).unwrap();
        assert!(
            !again
                .definitions
                .contains(&def("ship", "FromCache", Mode::Replaces))
        );
        assert!(
            again
                .definitions
                .contains(&def("outfit", "B", Mode::Replaces))
        );

        // A corrupt cache is a miss, never an error.
        fs::write(&cache, "not json").unwrap();
        assert_eq!(load_or_index(&plugin, &cache).unwrap(), again);
    }

    #[test]
    fn a_later_full_definition_overrides_an_earlier_one() {
        // Real case: Cromha Expansion and Factory.Outlets both define ship "Hornet".
        let a = index(&[def("ship", "Hornet", Mode::Replaces)], &[]);
        let b = index(&[def("ship", "Hornet", Mode::Replaces)], &[]);
        let overlaps = find_overlaps(&[
            PluginEntry {
                folder: "Factory.Outlets",
                index: &b,
            },
            PluginEntry {
                folder: "Cromha Expansion Plugin",
                index: &a,
            },
        ]);
        assert_eq!(
            overlaps,
            [Overlap {
                item: OverlapItem::Object {
                    kind: "ship".into(),
                    name: "Hornet".into()
                },
                winner: "Factory.Outlets".into(),
                overridden: vec!["Cromha Expansion Plugin".into()],
            }]
        );
    }

    #[test]
    fn layering_is_not_an_overlap_unless_a_replace_comes_after_it() {
        let ext = |n: &str| index(&[def("system", n, Mode::Extends)], &[]);
        let rep = |n: &str| index(&[def("system", n, Mode::Replaces)], &[]);

        // Two plugins that only add to the same system: no overlap at all.
        let (a, b) = (ext("1 Axis"), ext("1 Axis"));
        assert!(
            find_overlaps(&[
                PluginEntry {
                    folder: "bits",
                    index: &a
                },
                PluginEntry {
                    folder: "fauna",
                    index: &b
                },
            ])
            .is_empty()
        );

        // An extension loading AFTER the full definition layers on top: fine.
        let (base, extra) = (rep("X"), ext("X"));
        assert!(
            find_overlaps(&[
                PluginEntry {
                    folder: "a-base",
                    index: &base
                },
                PluginEntry {
                    folder: "b-extra",
                    index: &extra
                },
            ])
            .is_empty()
        );

        // But an extension loading BEFORE a full definition is wiped out by it.
        let overlaps = find_overlaps(&[
            PluginEntry {
                folder: "a-extra",
                index: &extra,
            },
            PluginEntry {
                folder: "b-base",
                index: &base,
            },
        ]);
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].winner, "b-base");
        assert_eq!(overlaps[0].overridden, ["a-extra"]);
    }

    #[test]
    fn shared_images_count_but_high_resolution_copies_do_not_clash_with_normal_ones() {
        let normal = index(&[], &["land/aera"]);
        let sharper = index(&[], &["land/aera@2x"]);
        let also_sharper = index(&[], &["land/aera@2x", "land/other@2x"]);
        // A high-resolution pack supplying `@2x` art for another plugin's normal art is by design.
        assert!(
            find_overlaps(&[
                PluginEntry {
                    folder: "base",
                    index: &normal
                },
                PluginEntry {
                    folder: "highdpi",
                    index: &sharper
                },
            ])
            .is_empty()
        );
        // Two plugins both supplying the same `@2x` art really do compete.
        let overlaps = find_overlaps(&[
            PluginEntry {
                folder: "High DPI",
                index: &sharper,
            },
            PluginEntry {
                folder: "landing.images.highres",
                index: &also_sharper,
            },
        ]);
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].item, OverlapItem::Image("land/aera@2x".into()));
        assert_eq!(overlaps[0].winner, "landing.images.highres");
        assert_eq!(overlaps[0].overridden, ["High DPI"]);
    }
}
