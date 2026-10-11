//! "Give it a name": adding a `name` line to the `plugin.txt` of a plugin that has none, so the
//! optional load-order feature (`crate::load_order`) can number its folder without the game
//! seeing a different plugin.
//!
//! # Why this exists
//!
//! The game knows a plugin by the `name` in its `plugin.txt`, or, when there is no `name`, by
//! its folder name. The load-order feature works by renaming folders (`010-Foo`), so it leaves
//! nameless plugins out: renaming their folder would rename the plugin as far as the game,
//! `plugins.txt`, profiles and other plugins' `requires` are concerned. Jon decided on
//! 2026-10-11 that such a plugin should instead be offered a name: either its folder name (the
//! default, which changes nothing about who the plugin is) or a name the user types. Once it
//! has a name it can be ordered like any other plugin.
//!
//! This is the first time the manager writes **inside** a plugin folder, so this module is
//! deliberately narrow about what it touches.
//!
//! # What is written, and where
//!
//! * One line, `name "<name>"`, added to the plugin's `plugin.txt`. Every other byte of the file
//!   stays exactly as it was. The line goes at the top (after a UTF-8 byte-order mark, if the
//!   file has one) and uses the file's own line ending (`\r\n` if its first line ends that way;
//!   *(inference)*). If the file's first line is indented, a line above it would turn it into a
//!   child of `name` and change what the file means, so in that one case the line goes at the
//!   end instead; either way the result is checked to read exactly like the old file plus the
//!   name. A plugin with no `plugin.txt` gets a new one holding just that line.
//! * The name is always written in quotes (`name "Foo"`, or in backticks if it contains a double
//!   quote) *(inference: matches how most plugins write it, and keeps a name starting with `#`
//!   from being read as a comment)*.
//! * The write is atomic (temporary file, then rename) and is read back: if the file doesn't
//!   then read back with exactly that name, the original bytes are put back (or the new file is
//!   removed) and nothing else happens.
//! * Before that, a copy of the file as it was is kept in the manager's own folder for this
//!   install, `<install state dir>/plugin-txt-backups/<folder>.plugin.txt` (or a short note,
//!   `<folder>.no-plugin-txt`, if there was no file), never inside the plugin folder. There is
//!   no "remove the name" button yet; that copy is what to restore by hand.
//! * The install record gets `added_name` (see `records::InstallRecord`), so the name is put
//!   back after every update ([`reapply_to_staged`], called from `manager`'s commit functions).
//!
//! # A typed name changes who the plugin is
//!
//! A name other than the folder name changes the plugin's identity. The manager moves what it
//! owns to the new name: the plugin's on/off entry in `plugins.txt` (keeping its value, so a
//! switched-off plugin stays off), its entries in every profile, and its install record. It
//! can't edit other plugins' files, so another plugin that `requires`, lists as `optional` or
//! `conflicts` with the old name stops matching it; the plan lists those so the review can warn.
//!
//! The steps run in this order, and a journal (`plugin-naming-journal.json`, beside
//! `records.json`) is written before the first one and removed after the last:
//!
//! 1. `plugins.txt` gets an entry for the new name with the old name's on/off value; the old
//!    entry stays. (Only when the name changes.)
//! 2. The name is written into `plugin.txt`.
//! 3. The install record gets the new identity and `added_name`; then (only when the name
//!    changes) the old `plugins.txt` entry is removed and profiles are moved to the new name.
//!
//! What a crash between steps leaves behind, and what [`recover`] (run every time the manager
//! reads the install) does about it:
//!
//! * Before or during step 1, or before step 2 finished: `plugin.txt` has no name, so the game
//!   still sees the old name, whose entry is untouched. Recovery undoes step 1 (puts the new
//!   name's entry back to what it was) and removes the journal. Nothing changed.
//! * After step 2: the game now sees the new name, whose entry (written in step 1) already has
//!   the right value, so a switched-off plugin is never switched on. Recovery finishes step 3.
//! * During step 3: each part of it can be done twice without harm, so recovery just does it.
//!
//! Recovery waits while the game runs (it writes `plugins.txt`); until then the journal stays and
//! new naming or load-order changes are refused. If the old name is also another installed
//! plugin's identity (two plugins claiming one name), its `plugins.txt` and profile entries are
//! left in place for that plugin and only copied to the new name.
//!
//! # Removing the feature
//!
//! This module belongs to the optional load-order feature; `crate::load_order`'s header lists
//! what to delete. Note that a plugin named here keeps depending on [`reapply_to_staged`] at
//! every update, so this file and its two hooks in `manager.rs` should stay while any install
//! record has an `added_name`.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::datanode::{self, Node};
use crate::files;
use crate::game_state::{self, GameProcess};
use crate::manager::{self, InstalledPlugin};
use crate::plugin_meta::PluginMeta;
use crate::plugin_state::PluginStates;
use crate::profiles::ProfileStore;
use crate::records::{self, InstallRecord};

/// The journal sits next to `records.json`.
pub const JOURNAL_FILE: &str = "plugin-naming-journal.json";
/// The load-order feature's journal (`load_order::JOURNAL_FILE`, checked equal by a test there).
/// Spelled out here so this file keeps working on its own if the rest of the feature is removed.
pub const LOAD_ORDER_JOURNAL_FILE: &str = "load-order-journal.json";
/// Copies of `plugin.txt` as it was before the manager added a name, beside `records.json`.
pub const BACKUP_DIR: &str = "plugin-txt-backups";
/// Longest name accepted, in bytes. Real plugin names are a few dozen.
pub const MAX_NAME_BYTES: usize = 200;
const PLUGIN_TXT: &str = "plugin.txt";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Why a name can't be used. The messages are shown to the user as they are.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("The name is empty.")]
    Blank,
    #[error("The name contains a tab, a line break or another invisible control character.")]
    ControlCharacter,
    #[error("The name is {0} bytes long; the limit is {MAX_NAME_BYTES}.")]
    TooLong(usize),
    #[error(
        "The name contains both a double quote (\") and a backtick (`); plugin.txt can't hold a name with both."
    )]
    BothQuoteKinds,
    #[error("Another installed plugin (in the folder {folder:?}) is already called that.")]
    Taken { folder: String },
}

#[derive(Debug, thiserror::Error)]
pub enum NamingError {
    #[error("{0}")]
    Name(#[from] NameError),
    #[error("The folder name can't be used as the name: {0}")]
    FolderNameUnusable(NameError),
    #[error("The plugin folder {0:?} is no longer in plugins/.")]
    Missing(String),
    #[error(
        "The manager has no record of {0:?} (it was added by hand and isn't linked to a catalog plugin), so it won't write into it."
    )]
    NotLinked(String),
    #[error("{folder:?} already has a name in its plugin.txt ({name:?}).")]
    AlreadyNamed { folder: String, name: String },
    #[error(
        "The plugin.txt in {0:?} already has a `name` line with nothing usable after it. The manager won't guess what was meant: edit that line by hand (or delete it), then try again."
    )]
    BlankNameLine(String),
    #[error("The plugin.txt in {0:?} isn't plain UTF-8 text, so the manager won't change it.")]
    Unreadable(String),
    #[error(
        "Adding a name line to the plugin.txt in {0:?} would change what the rest of the file means, so the manager won't do it; add the line by hand."
    )]
    WouldChangeMeaning(String),
    #[error("Endless Sky is running; close it first")]
    GameRunning,
    #[error(
        "An earlier change to the plugin folders is still being tidied up. The manager finishes it by itself the next time it reads your plugins (after Endless Sky is closed); try again in a moment."
    )]
    Busy,
    #[error("The plugin.txt in {0:?} changed since you reviewed this. Review it again.")]
    Changed(String),
    #[error(
        "The name didn't read back from plugin.txt as written, so the file was put back as it was. ({0})"
    )]
    ReadBackFailed(String),
    #[error("{0}")]
    Io(String),
}

impl From<io::Error> for NamingError {
    fn from(e: io::Error) -> Self {
        NamingError::Io(e.to_string())
    }
}

fn io_err(e: impl std::fmt::Display) -> NamingError {
    NamingError::Io(e.to_string())
}

// ---------------------------------------------------------------------------
// The name itself
// ---------------------------------------------------------------------------

/// Checks a name and returns it with surrounding whitespace trimmed (the form that is written,
/// and the form a review should show).
///
/// `own_folder` is the plugin being named: its folder name is always allowed (it is already the
/// plugin's identity, so using it changes nothing), and its own entry in `installed` is never
/// counted as "another plugin".
pub fn validate_name(
    raw: &str,
    own_folder: &str,
    installed: &[InstalledPlugin],
) -> Result<String, NameError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(NameError::Blank);
    }
    if name.chars().any(char::is_control) {
        return Err(NameError::ControlCharacter);
    }
    if name.len() > MAX_NAME_BYTES {
        return Err(NameError::TooLong(name.len()));
    }
    // `datanode::quote` wraps a name holding `"` in backticks and one holding a backtick in
    // double quotes; a name holding both can't be written as one token.
    if name.contains('"') && name.contains('`') {
        return Err(NameError::BothQuoteKinds);
    }
    if name != own_folder
        && let Some(other) = installed
            .iter()
            .find(|p| p.folder != own_folder && p.identity == name)
    {
        return Err(NameError::Taken {
            folder: other.folder.clone(),
        });
    }
    Ok(name.to_string())
}

/// The exact line written into `plugin.txt`, e.g. `name "Mega Freight"`.
pub fn name_line(name: &str) -> String {
    let quoted = datanode::quote(name);
    // `quote` leaves a plain word bare; always quote it, so a name starting with `#` is not
    // read as a comment and the line looks like most plugins' own.
    let token = if quoted == name {
        format!("\"{name}\"")
    } else {
        quoted
    };
    format!("name {token}")
}

/// What a `plugin.txt` says about the plugin's name.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Naming {
    /// There is no `plugin.txt`.
    NoFile,
    /// There is one, without a top-level `name` line.
    NoName,
    /// It names the plugin (the last top-level `name` with a value wins, as in the game).
    Named(String),
    /// It has a top-level `name` line with no value or only whitespace: refused, never guessed.
    BlankNameLine,
}

fn naming_of(text: Option<&str>) -> Naming {
    let Some(text) = text else {
        return Naming::NoFile;
    };
    if let Some(name) = PluginMeta::parse(text).name
        && !name.trim().is_empty()
    {
        return Naming::Named(name);
    }
    if datanode::parse(text).iter().any(|n| n.key() == "name") {
        Naming::BlankNameLine
    } else {
        Naming::NoName
    }
}

/// The new contents of `plugin.txt`: `original` (or nothing, if there was no file) with the
/// name line added, every other byte unchanged. Pure; see the module docs for where the line
/// goes. Fails if the result would not read as the old file plus the name.
pub fn with_name_line(original: Option<&[u8]>, name: &str) -> Result<Vec<u8>, NamingError> {
    let line = name_line(name);
    let Some(original) = original else {
        return Ok(format!("{line}\n").into_bytes());
    };
    let text =
        std::str::from_utf8(original).map_err(|_| NamingError::Unreadable(PLUGIN_TXT.into()))?;
    let (bom, body) = match text.strip_prefix('\u{feff}') {
        Some(rest) => ("\u{feff}", rest),
        None => ("", text),
    };
    let eol = match body.find('\n') {
        Some(i) if body[..i].ends_with('\r') => "\r\n",
        _ => "\n",
    };
    let before = datanode::parse(body);
    let name_node = Node::new(["name", name]);

    let at_top = format!("{bom}{line}{eol}{body}");
    let mut expected = vec![name_node.clone()];
    expected.extend(before.iter().cloned());
    if datanode::parse(&at_top) == expected {
        return Ok(at_top.into_bytes());
    }

    // The first line is indented: put the line at the end instead.
    let separator = if body.is_empty() || body.ends_with('\n') {
        ""
    } else {
        eol
    };
    let at_end = format!("{bom}{body}{separator}{line}{eol}");
    let mut expected = before;
    expected.push(name_node);
    if datanode::parse(&at_end) == expected {
        return Ok(at_end.into_bytes());
    }
    Err(NamingError::WouldChangeMeaning(PLUGIN_TXT.into()))
}

/// `plugin.txt` in `plugin_dir`, as bytes; `None` if there is none.
fn read_plugin_txt(plugin_dir: &Path) -> Result<Option<Vec<u8>>, NamingError> {
    match fs::read(plugin_dir.join(PLUGIN_TXT)) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// What the file in `folder` says about its name; `folder` is only used in error messages.
fn naming_of_bytes(bytes: Option<&[u8]>, folder: &str) -> Result<Naming, NamingError> {
    match bytes {
        None => Ok(Naming::NoFile),
        Some(bytes) => std::str::from_utf8(bytes)
            .map(|text| naming_of(Some(text)))
            .map_err(|_| NamingError::Unreadable(folder.to_string())),
    }
}

type WriteFn<'a> = &'a dyn Fn(&Path, &[u8]) -> io::Result<()>;

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    files::write_atomic(path, bytes)
}

/// Writes the name into `plugin_dir/plugin.txt` (whose current bytes are `original`) and reads
/// it back. If it doesn't read back with exactly `name`, the original is put back.
fn write_name(plugin_dir: &Path, original: Option<&[u8]>, name: &str) -> Result<(), NamingError> {
    write_name_with(plugin_dir, original, name, &write_atomic)
}

fn write_name_with(
    plugin_dir: &Path,
    original: Option<&[u8]>,
    name: &str,
    write: WriteFn,
) -> Result<(), NamingError> {
    let path = plugin_dir.join(PLUGIN_TXT);
    let contents = with_name_line(original, name)?;
    // A temporary file is made beside plugin.txt; the atomic write leaves it with owner-only
    // permissions on Unix, so give it back the old file's (best effort).
    let permissions = fs::metadata(&path).ok().map(|m| m.permissions());
    write(&path, &contents)?;
    if let Some(permissions) = permissions {
        let _ = fs::set_permissions(&path, permissions);
    }
    let read_back = fs::read(&path)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| PluginMeta::parse(&text).name);
    if read_back.as_deref() == Some(name) {
        return Ok(());
    }
    let put_back = match original {
        Some(bytes) => files::write_atomic(&path, bytes),
        None => fs::remove_file(&path),
    };
    Err(NamingError::ReadBackFailed(match put_back {
        Ok(()) => format!("read back {read_back:?}"),
        Err(e) => format!(
            "read back {read_back:?}, and putting the old file back failed too: {e}; a copy is in the manager's plugin-txt-backups folder"
        ),
    }))
}

// ---------------------------------------------------------------------------
// Where things are
// ---------------------------------------------------------------------------

/// Where things are, for one game config dir.
#[derive(Debug, Clone, Copy)]
pub struct Paths<'a> {
    pub config_dir: &'a Path,
    /// The install's `records.json`; the journal and the backups are kept beside it.
    pub records_path: &'a Path,
    pub profiles_path: &'a Path,
}

impl Paths<'_> {
    fn plugins_dir(&self) -> PathBuf {
        crate::install::plugins_dir(self.config_dir)
    }

    fn journal_path(&self) -> PathBuf {
        self.records_path.with_file_name(JOURNAL_FILE)
    }

    fn load_order_journal_path(&self) -> PathBuf {
        self.records_path.with_file_name(LOAD_ORDER_JOURNAL_FILE)
    }
}

/// The folder holding the copies of `plugin.txt`, beside `records_path`.
pub fn backup_dir(records_path: &Path) -> PathBuf {
    records_path.with_file_name(BACKUP_DIR)
}

/// The backup for a plugin, named after its original folder name (the one it had before any
/// load-order number), so the same plugin keeps one backup however its folder is numbered.
fn backup_path(backup_dir: &Path, record: &InstallRecord, had_file: bool) -> PathBuf {
    let key = crate::install::sanitize_folder_name(
        record.original_folder.as_deref().unwrap_or(&record.folder),
    );
    backup_dir.join(if had_file {
        format!("{key}.plugin.txt")
    } else {
        format!("{key}.no-plugin-txt")
    })
}

/// Saves what `plugin.txt` looked like before the name is added: a copy, or a note that there
/// was none. The other kind of file for the same plugin is removed so only the current one is
/// there.
fn write_backup(path: &Path, original: Option<&[u8]>) -> Result<(), NamingError> {
    let note;
    let contents: &[u8] = match original {
        Some(bytes) => bytes,
        None => {
            note = "This plugin had no plugin.txt before Endless Sky Mod Manager added one with a \
                    name line. To undo it, delete the plugin's plugin.txt.\n"
                .to_string();
            note.as_bytes()
        }
    };
    files::write_atomic(path, contents)?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let other = if let Some(key) = file_name.strip_suffix(".plugin.txt") {
        Some(format!("{key}.no-plugin-txt"))
    } else {
        file_name
            .strip_suffix(".no-plugin-txt")
            .map(|key| format!("{key}.plugin.txt"))
    };
    if let Some(other) = other {
        let _ = fs::remove_file(path.with_file_name(other));
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A plain folder name (one path component), so it is safe to join onto `plugins/`.
fn is_plain_folder_name(folder: &str) -> bool {
    let mut parts = Path::new(folder).components();
    matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none()
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

/// The name the user chose in the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameChoice {
    /// The default: the folder name, which is already the plugin's identity.
    FolderName,
    /// A name the user typed (trimmed and checked by [`validate_name`]).
    Typed(String),
}

/// How another plugin's `plugin.txt` mentions the plugin being named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Relation {
    Requires,
    Optional,
    Conflicts,
}

/// Another installed plugin that names the plugin's old identity in its dependencies. The
/// manager can't change other plugins' files, so after a rename these stop matching.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Dependent {
    pub plugin: String,
    pub folder: String,
    pub relation: Relation,
}

/// Exactly what [`commit_name`] would do. Planning changes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamingPlan {
    pub folder: String,
    /// The name to write, already trimmed.
    pub name: String,
    /// What the game calls the plugin now: its folder name.
    pub old_identity: String,
    /// The exact line added to `plugin.txt`.
    pub line: String,
    /// SHA-256 of `plugin.txt` when planned; `None` if there was none (one will be created).
    /// The commit refuses if the file has changed since.
    pub original_sha256: Option<String>,
    /// Where the copy of `plugin.txt` as it was (or the note that there was none) will be kept.
    pub backup_path: PathBuf,
    /// Whether the plugin is switched on now (its setting carries over to a new name).
    pub enabled: bool,
    /// Profiles that list the plugin under its old identity (only when the identity changes).
    pub profiles: Vec<String>,
    /// Other plugins whose dependencies mention the old identity (only when it changes).
    pub dependents: Vec<Dependent>,
}

impl NamingPlan {
    /// True for a typed name that differs from the folder name.
    pub fn identity_changes(&self) -> bool {
        self.name != self.old_identity
    }

    /// True when the plugin has no `plugin.txt` yet.
    pub fn creates_file(&self) -> bool {
        self.original_sha256.is_none()
    }
}

/// Plans adding `choice` as the name of the plugin in `folder`. Refuses a plugin the manager
/// has no record of, one that already has a name, and one whose `plugin.txt` has a blank `name`
/// line (which the user should fix by hand rather than have guessed at).
pub fn plan_name(
    paths: &Paths,
    installed: &[InstalledPlugin],
    states: &PluginStates,
    folder: &str,
    choice: &NameChoice,
) -> Result<NamingPlan, NamingError> {
    let plugin = installed
        .iter()
        .find(|p| p.folder == folder)
        .ok_or_else(|| NamingError::Missing(folder.to_string()))?;
    let record = plugin
        .record
        .as_ref()
        .ok_or_else(|| NamingError::NotLinked(folder.to_string()))?;
    if !is_plain_folder_name(folder) {
        return Err(NamingError::Missing(folder.to_string()));
    }
    let original = read_plugin_txt(&paths.plugins_dir().join(folder))?;
    match naming_of_bytes(original.as_deref(), folder)? {
        Naming::Named(name) => {
            return Err(NamingError::AlreadyNamed {
                folder: folder.to_string(),
                name,
            });
        }
        Naming::BlankNameLine => return Err(NamingError::BlankNameLine(folder.to_string())),
        Naming::NoFile | Naming::NoName => {}
    }
    let name = match choice {
        NameChoice::FolderName => {
            validate_name(folder, folder, installed).map_err(NamingError::FolderNameUnusable)?
        }
        NameChoice::Typed(raw) => validate_name(raw, folder, installed)?,
    };
    // Make sure the line can be added to this particular file before offering it.
    with_name_line(original.as_deref(), &name).map_err(|e| match e {
        NamingError::Unreadable(_) => NamingError::Unreadable(folder.to_string()),
        NamingError::WouldChangeMeaning(_) => NamingError::WouldChangeMeaning(folder.to_string()),
        other => other,
    })?;

    let old_identity = plugin.identity.clone();
    let changes = name != old_identity;
    let profiles = if changes {
        ProfileStore::load(paths.profiles_path)
            .map_err(io_err)?
            .profiles
            .iter()
            .filter(|(_, p)| p.enabled.contains_key(&old_identity))
            .map(|(profile_name, _)| profile_name.clone())
            .collect()
    } else {
        Vec::new()
    };
    let dependents = if changes {
        dependents_of(&old_identity, folder, installed)
    } else {
        Vec::new()
    };
    Ok(NamingPlan {
        folder: folder.to_string(),
        line: name_line(&name),
        enabled: game_state::effective_enabled(states, &old_identity),
        backup_path: backup_path(&backup_dir(paths.records_path), record, original.is_some()),
        original_sha256: original.as_deref().map(sha256_hex),
        name,
        old_identity,
        profiles,
        dependents,
    })
}

/// Every other installed plugin (on or off) whose `plugin.txt` mentions `identity` as a
/// requirement, an optional plugin or a conflict.
fn dependents_of(
    identity: &str,
    own_folder: &str,
    installed: &[InstalledPlugin],
) -> Vec<Dependent> {
    let mut found = BTreeSet::new();
    for p in installed.iter().filter(|p| p.folder != own_folder) {
        let deps = &p.meta.dependencies;
        for (list, relation) in [
            (&deps.requires, Relation::Requires),
            (&deps.optional, Relation::Optional),
            (&deps.conflicts, Relation::Conflicts),
        ] {
            if list.contains(identity) {
                found.insert(Dependent {
                    plugin: p.identity.clone(),
                    folder: p.folder.clone(),
                    relation,
                });
            }
        }
    }
    found.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Committing, and finishing or undoing an interrupted commit
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Journal {
    format: u32,
    folder: String,
    name: String,
    old_identity: String,
    /// What `plugins.txt` said for the new name before step 1 (`None`: nothing), so an undo can
    /// put it back exactly.
    new_entry_before: Option<bool>,
}

impl Journal {
    fn identity_changes(&self) -> bool {
        self.name != self.old_identity
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamingOutcome {
    pub backup_path: PathBuf,
    /// Where `plugins.txt` was copied before it was changed (only when the identity changed).
    pub plugin_states_backup: Option<PathBuf>,
    /// `plugins.txt` was written while the game's process list couldn't be read.
    pub game_detection_failed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamingRecovery {
    /// There was no interrupted naming.
    Nothing,
    /// The name had been written; the rest was finished.
    Finished { folder: String, name: String },
    /// The name had not been written; the little that was done was undone.
    RolledBack { folder: String },
}

fn write_states(
    config_dir: &Path,
    states: &PluginStates,
    game: GameProcess,
) -> Result<game_state::WriteOutcome, NamingError> {
    game_state::write_plugin_states(config_dir, states, game).map_err(|e| match e {
        game_state::WriteError::GameRunning => NamingError::GameRunning,
        game_state::WriteError::Io(e) => e.into(),
    })
}

fn write_journal(path: &Path, journal: &Journal) -> Result<(), NamingError> {
    let mut json = serde_json::to_vec_pretty(journal).map_err(io_err)?;
    json.push(b'\n');
    files::write_atomic(path, &json)?;
    Ok(())
}

fn remove_journal(path: &Path) -> Result<(), NamingError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(NamingError::Io(format!(
            "the change is complete, but its journal couldn't be removed: {e}"
        ))),
    }
}

/// Carries out `plan`: see the module docs for the steps. Refuses while the game runs, while
/// an earlier naming or load-order change is unfinished, and if the folder or its `plugin.txt`
/// changed since `plan` was made.
pub fn commit_name(
    paths: &Paths,
    plan: &NamingPlan,
    game: GameProcess,
) -> Result<NamingOutcome, NamingError> {
    commit_name_with(paths, plan, game, &write_atomic)
}

fn commit_name_with(
    paths: &Paths,
    plan: &NamingPlan,
    game: GameProcess,
    write: WriteFn,
) -> Result<NamingOutcome, NamingError> {
    if game == GameProcess::Running {
        return Err(NamingError::GameRunning);
    }
    let _lock = files::plugin_folders_lock();
    let journal_path = paths.journal_path();
    if journal_path.exists() || paths.load_order_journal_path().exists() {
        return Err(NamingError::Busy);
    }
    if !is_plain_folder_name(&plan.folder) {
        return Err(NamingError::Missing(plan.folder.clone()));
    }
    let dir = paths.plugins_dir().join(&plan.folder);
    if !dir.is_dir() {
        return Err(NamingError::Missing(plan.folder.clone()));
    }
    // Looked at again: the plan may be a little old.
    let original = read_plugin_txt(&dir)?;
    if original.as_deref().map(sha256_hex) != plan.original_sha256 {
        return Err(NamingError::Changed(plan.folder.clone()));
    }
    if !matches!(
        naming_of_bytes(original.as_deref(), &plan.folder)?,
        Naming::NoFile | Naming::NoName
    ) {
        return Err(NamingError::Changed(plan.folder.clone()));
    }
    let records = records::load(paths.records_path).map_err(io_err)?;
    if !records.contains_key(&plan.folder) {
        return Err(NamingError::NotLinked(plan.folder.clone()));
    }

    write_backup(&plan.backup_path, original.as_deref())?;
    let mut states = game_state::read_plugin_states(paths.config_dir)?;
    let journal = Journal {
        format: 1,
        folder: plan.folder.clone(),
        name: plan.name.clone(),
        old_identity: plan.old_identity.clone(),
        new_entry_before: states.get(&plan.name).copied(),
    };
    write_journal(&journal_path, &journal)?;

    // Step 1: the new name gets the old name's on/off value; the old entry stays.
    let mut outcome = game_state::WriteOutcome {
        backup: None,
        detection_failed: false,
    };
    if journal.identity_changes() {
        let on = game_state::effective_enabled(&states, &journal.old_identity);
        states.insert(journal.name.clone(), on);
        match write_states(paths.config_dir, &states, game) {
            Ok(written) => outcome = written,
            Err(e) => {
                // Nothing reached the plugin; undo whatever part of step 1 landed.
                let _ = roll_back(paths, &journal, game);
                return Err(e);
            }
        }
    }

    // Step 2: the name itself.
    if let Err(e) = write_name_with(&dir, original.as_deref(), &journal.name, write) {
        let _ = roll_back(paths, &journal, game);
        return Err(e);
    }

    // Step 3: records, the old entry and profiles. A failure here leaves the journal, so the
    // next read of the install finishes it.
    finish(paths, &journal, game)?;
    remove_journal(&journal_path)?;
    tracing::info!(
        folder = journal.folder,
        name = journal.name,
        identity_changed = journal.identity_changes(),
        "added a name to a plugin's plugin.txt"
    );
    Ok(NamingOutcome {
        backup_path: plan.backup_path.clone(),
        plugin_states_backup: outcome.backup,
        game_detection_failed: outcome.detection_failed,
    })
}

/// Step 3 (also what recovery does after a crash once the name is on disk). Every part can run
/// twice without harm.
fn finish(paths: &Paths, journal: &Journal, game: GameProcess) -> Result<(), NamingError> {
    let mut records = records::load(paths.records_path).map_err(io_err)?;
    if let Some(record) = records.get_mut(&journal.folder) {
        record.identity = journal.name.clone();
        record.added_name = Some(journal.name.clone());
        records::save(paths.records_path, &records).map_err(io_err)?;
    }
    if !journal.identity_changes() {
        return Ok(());
    }
    // If another installed plugin also goes by the old name, its entries stay where they are.
    let shared = manager::scan_installed(&paths.plugins_dir(), &records)?
        .iter()
        .any(|p| p.folder != journal.folder && p.identity == journal.old_identity);

    let mut states = game_state::read_plugin_states(paths.config_dir)?;
    let before = states.clone();
    if let Some(on) = states.get(&journal.old_identity).copied() {
        // Normally already there from step 1.
        states.entry(journal.name.clone()).or_insert(on);
        if !shared {
            states.remove(&journal.old_identity);
        }
    }
    if states != before {
        write_states(paths.config_dir, &states, game)?;
    }

    let mut store = ProfileStore::load(paths.profiles_path).map_err(io_err)?;
    let mut changed = false;
    for profile in store.profiles.values_mut() {
        if let Some(catalog_name) = profile.enabled.get(&journal.old_identity).cloned() {
            profile
                .enabled
                .entry(journal.name.clone())
                .or_insert(catalog_name);
            if !shared {
                profile.enabled.remove(&journal.old_identity);
            }
            changed = true;
        }
    }
    if changed {
        store.save(paths.profiles_path).map_err(io_err)?;
    }
    Ok(())
}

/// Undoes step 1 (the only thing that happens before the name is written) and removes the
/// journal.
fn roll_back(paths: &Paths, journal: &Journal, game: GameProcess) -> Result<(), NamingError> {
    if journal.identity_changes() {
        let mut states = game_state::read_plugin_states(paths.config_dir)?;
        let before = states.clone();
        match journal.new_entry_before {
            Some(on) => states.insert(journal.name.clone(), on),
            None => states.remove(&journal.name),
        };
        if states != before {
            write_states(paths.config_dir, &states, game)?;
        }
    }
    remove_journal(&paths.journal_path())
}

/// Finishes or undoes a naming that was interrupted (a crash, a power cut): if the name made it
/// into `plugin.txt` the rest is finished, otherwise the little that was done is undone. Call it
/// before the plugin folders are read. Waits (returns `GameRunning`, leaving the journal) while
/// the game runs, since it may write `plugins.txt`.
pub fn recover(paths: &Paths, game: GameProcess) -> Result<NamingRecovery, NamingError> {
    let _lock = files::plugin_folders_lock();
    let journal_path = paths.journal_path();
    let text = match fs::read_to_string(&journal_path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(NamingRecovery::Nothing),
        Err(e) => return Err(e.into()),
    };
    let journal: Journal = serde_json::from_str(&text).map_err(|e| {
        NamingError::Io(format!(
            "the naming journal is unreadable ({e}); it was left in place"
        ))
    })?;
    if journal.format > 1 {
        return Err(NamingError::Io(
            "the naming journal was written by a newer version of the manager; it was left in place"
                .into(),
        ));
    }
    if !is_plain_folder_name(&journal.folder) {
        return Err(NamingError::Io(format!(
            "the naming journal names {:?}, which isn't a plain folder name; it was left in place",
            journal.folder
        )));
    }
    if game == GameProcess::Running {
        return Err(NamingError::GameRunning);
    }
    let named = read_plugin_txt(&paths.plugins_dir().join(&journal.folder))?
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| PluginMeta::parse(&text).name)
        .is_some_and(|name| name == journal.name);
    if named {
        finish(paths, &journal, game)?;
        remove_journal(&journal_path)?;
        tracing::warn!(
            folder = journal.folder,
            "finished an interrupted plugin naming"
        );
        Ok(NamingRecovery::Finished {
            folder: journal.folder,
            name: journal.name,
        })
    } else {
        roll_back(paths, &journal, game)?;
        tracing::warn!(
            folder = journal.folder,
            "undid an interrupted plugin naming"
        );
        Ok(NamingRecovery::RolledBack {
            folder: journal.folder,
        })
    }
}

// ---------------------------------------------------------------------------
// Updates: putting the name back
// ---------------------------------------------------------------------------

/// For planning an update or reinstall: the identity the new copy will have once
/// [`reapply_to_staged`] has run. `Some(name)` when `record` says the manager added a name and
/// the downloaded copy (`staged_meta`) has none of its own; `None` otherwise (then the copy's
/// own `plugin.txt` decides, as for any plugin).
pub fn name_to_reapply(record: Option<&InstallRecord>, staged_meta: &PluginMeta) -> Option<String> {
    let name = record?.added_name.as_ref()?;
    staged_meta
        .name
        .as_deref()
        .is_none_or(|n| n.trim().is_empty())
        .then(|| name.clone())
}

/// For the installer, before a downloaded copy (`staged_root`, still in the manager's staging
/// folder) replaces a plugin: if the plugin's `record` says the manager added a name and the new
/// copy has no `name` of its own, writes the same name into the copy, so it arrives in
/// `plugins/` already named and its identity never changes. Returns what the new record's
/// `added_name` should be:
///
/// * `Some(name)` when the name was written again;
/// * `None` when there is nothing to put back, or when the new version now names itself (from
///   then on its own name is its identity, as for any plugin; if that differs from the name the
///   manager added, the plugin's identity changes upstream-side, which is out of this module's
///   hands, exactly as when any plugin renames itself).
///
/// A copy whose `plugin.txt` has a blank `name` line is refused rather than guessed at, before
/// anything is installed.
pub fn reapply_to_staged(
    staged_root: &Path,
    record: Option<&InstallRecord>,
    backup_dir: &Path,
) -> Result<Option<String>, NamingError> {
    let Some(record) = record else {
        return Ok(None);
    };
    let Some(name) = record.added_name.as_deref() else {
        return Ok(None);
    };
    let original = read_plugin_txt(staged_root)?;
    match naming_of_bytes(original.as_deref(), &record.folder)? {
        Naming::Named(_) => Ok(None),
        Naming::BlankNameLine => Err(NamingError::Io(format!(
            "The new version of {:?} has a `name` line with no name in its plugin.txt, so the manager can't put back the name {name:?} it added earlier. Nothing was installed.",
            record.folder
        ))),
        Naming::NoFile | Naming::NoName => {
            write_backup(
                &backup_path(backup_dir, record, original.is_some()),
                original.as_deref(),
            )?;
            write_name(staged_root, original.as_deref(), name)?;
            Ok(Some(name.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::Profile;
    use crate::records::InstallRecords;

    // --- helpers ---------------------------------------------------------

    /// A throwaway game config dir with `plugins/`, and the manager's files beside it.
    struct World {
        _dir: tempfile::TempDir,
        config: PathBuf,
        records_path: PathBuf,
        profiles_path: PathBuf,
    }

    impl World {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let config = dir.path().join("endless-sky");
            fs::create_dir_all(config.join("plugins")).unwrap();
            World {
                records_path: dir.path().join("app-data/records.json"),
                profiles_path: dir.path().join("app-data/profiles.json"),
                _dir: dir,
                config,
            }
        }

        fn paths(&self) -> Paths<'_> {
            Paths {
                config_dir: &self.config,
                records_path: &self.records_path,
                profiles_path: &self.profiles_path,
            }
        }

        fn dir(&self, folder: &str) -> PathBuf {
            self.config.join("plugins").join(folder)
        }

        /// A plugin folder with a record; `plugin_txt` is the exact file content (`None`: no
        /// file).
        fn add(&self, folder: &str, plugin_txt: Option<&str>) {
            let dir = self.dir(folder);
            fs::create_dir_all(dir.join("data")).unwrap();
            if let Some(text) = plugin_txt {
                fs::write(dir.join(PLUGIN_TXT), text).unwrap();
            }
            let identity = plugin_txt
                .and_then(|t| PluginMeta::parse(t).name)
                .unwrap_or_else(|| folder.to_string());
            let mut records = self.records();
            records.insert(
                folder.to_string(),
                InstallRecord {
                    catalog_name: folder.to_string(),
                    folder: folder.to_string(),
                    identity,
                    version: "1".into(),
                    source_url: "https://example.com/x.zip".into(),
                    sha256: "hash".into(),
                    original_folder: None,
                    added_name: None,
                },
            );
            records::save(&self.records_path, &records).unwrap();
        }

        /// A plugin folder the manager knows nothing about.
        fn add_unlinked(&self, folder: &str, plugin_txt: Option<&str>) {
            let dir = self.dir(folder);
            fs::create_dir_all(dir.join("data")).unwrap();
            if let Some(text) = plugin_txt {
                fs::write(dir.join(PLUGIN_TXT), text).unwrap();
            }
        }

        fn records(&self) -> InstallRecords {
            records::load(&self.records_path).unwrap()
        }

        fn installed(&self) -> Vec<InstalledPlugin> {
            manager::scan_installed(&self.config.join("plugins"), &self.records()).unwrap()
        }

        fn states(&self) -> PluginStates {
            game_state::read_plugin_states(&self.config).unwrap()
        }

        fn set_states(&self, states: &[(&str, bool)]) {
            let states: PluginStates = states.iter().map(|(n, v)| (n.to_string(), *v)).collect();
            game_state::write_plugin_states(&self.config, &states, GameProcess::NotRunning)
                .unwrap();
        }

        fn profiles(&self) -> ProfileStore {
            ProfileStore::load(&self.profiles_path).unwrap()
        }

        fn plugin_txt(&self, folder: &str) -> Option<Vec<u8>> {
            fs::read(self.dir(folder).join(PLUGIN_TXT)).ok()
        }

        fn plan(&self, folder: &str, choice: NameChoice) -> Result<NamingPlan, NamingError> {
            plan_name(
                &self.paths(),
                &self.installed(),
                &self.states(),
                folder,
                &choice,
            )
        }

        fn name(&self, folder: &str, choice: NameChoice) -> NamingOutcome {
            let plan = self.plan(folder, choice).unwrap();
            commit_name(&self.paths(), &plan, GameProcess::NotRunning).unwrap()
        }

        fn journal_exists(&self) -> bool {
            self.paths().journal_path().exists()
        }
    }

    fn typed(name: &str) -> NameChoice {
        NameChoice::Typed(name.to_string())
    }

    fn profile(entries: &[(&str, Option<&str>)]) -> Profile {
        Profile {
            enabled: entries
                .iter()
                .map(|(i, c)| (i.to_string(), c.map(str::to_string)))
                .collect(),
        }
    }

    // --- validation ------------------------------------------------------

    #[test]
    fn a_name_is_trimmed_and_checked() {
        let w = World::new();
        w.add("Foo", None);
        w.add("Other", Some("name \"Taken Name\"\n"));
        let installed = w.installed();
        let check = |raw: &str| validate_name(raw, "Foo", &installed);

        assert_eq!(check("  My Plugin \t"), Ok("My Plugin".to_string()));
        assert_eq!(check("   "), Err(NameError::Blank));
        assert_eq!(check(""), Err(NameError::Blank));
        assert_eq!(check("a\nb"), Err(NameError::ControlCharacter));
        assert_eq!(check("a\tb"), Err(NameError::ControlCharacter));
        assert_eq!(check("a\u{7}b"), Err(NameError::ControlCharacter));
        assert_eq!(check(&"x".repeat(200)), Ok("x".repeat(200)));
        assert_eq!(check(&"x".repeat(201)), Err(NameError::TooLong(201)));
        // 67 three-byte characters are 201 bytes.
        assert_eq!(check(&"€".repeat(67)), Err(NameError::TooLong(201)));
        assert_eq!(check("say \"hi\""), Ok("say \"hi\"".to_string()));
        assert_eq!(check("tick`"), Ok("tick`".to_string()));
        assert_eq!(check("both \" and `"), Err(NameError::BothQuoteKinds));
        assert_eq!(
            check("Taken Name"),
            Err(NameError::Taken {
                folder: "Other".into()
            })
        );
        // Exact and case-sensitive.
        assert_eq!(check("taken name"), Ok("taken name".to_string()));
    }

    #[test]
    fn the_plugins_own_folder_name_is_always_allowed_even_if_another_plugin_shares_it() {
        let w = World::new();
        w.add("Foo", None);
        // Another plugin whose plugin.txt also calls itself "Foo" (a clash that already exists).
        w.add_unlinked("Bar", Some("name \"Foo\"\n"));
        let installed = w.installed();
        assert_eq!(validate_name("Foo", "Foo", &installed), Ok("Foo".into()));
        assert_eq!(validate_name(" Foo ", "Foo", &installed), Ok("Foo".into()));
    }

    #[test]
    fn the_folder_name_option_is_refused_when_the_folder_name_itself_is_not_a_valid_name() {
        let w = World::new();
        // 201 bytes, but only 67 characters, so the path stays short enough for Windows.
        let long = "€".repeat(67);
        w.add(&long, None);
        let err = w.plan(&long, NameChoice::FolderName).unwrap_err();
        assert!(
            matches!(
                err,
                NamingError::FolderNameUnusable(NameError::TooLong(201))
            ),
            "{err:?}"
        );
        // A typed name still works.
        assert_eq!(w.plan(&long, typed("Short")).unwrap().name, "Short");
    }

    // --- writing the line ------------------------------------------------

    #[test]
    fn the_line_is_always_quoted_and_round_trips() {
        assert_eq!(name_line("Foo"), "name \"Foo\"");
        assert_eq!(name_line("Mega Freight"), "name \"Mega Freight\"");
        assert_eq!(name_line("Jimmy's"), "name \"Jimmy's\"");
        assert_eq!(name_line("say \"hi\""), "name `say \"hi\"`");
        assert_eq!(name_line("tick`"), "name \"tick`\"");
        for name in [
            "Foo",
            "#not a comment",
            "say \"hi\"",
            "tick`",
            "a#b",
            "€uro",
        ] {
            let text = String::from_utf8(with_name_line(None, name).unwrap()).unwrap();
            assert_eq!(
                PluginMeta::parse(&text).name.as_deref(),
                Some(name),
                "{name}"
            );
        }
    }

    #[test]
    fn a_missing_file_becomes_just_the_line() {
        assert_eq!(with_name_line(None, "Foo").unwrap(), b"name \"Foo\"\n");
    }

    #[test]
    fn the_line_goes_on_top_and_every_other_byte_is_kept() {
        let original = "about \"An old plugin\"\nversion 1.2\ndependencies\n\trequires\n\t\t\"Base\"\n# a comment\n";
        let new = with_name_line(Some(original.as_bytes()), "My Plugin").unwrap();
        assert_eq!(
            String::from_utf8(new.clone()).unwrap(),
            format!("name \"My Plugin\"\n{original}")
        );
        let meta = PluginMeta::parse(std::str::from_utf8(&new).unwrap());
        assert_eq!(meta.name.as_deref(), Some("My Plugin"));
        assert_eq!(meta.version.as_deref(), Some("1.2"));
        assert!(meta.dependencies.requires.contains("Base"));
    }

    #[test]
    fn crlf_files_get_a_crlf_line() {
        let original = b"about \"x\"\r\nversion 1\r\n";
        let new = with_name_line(Some(original), "Foo").unwrap();
        assert_eq!(new, b"name \"Foo\"\r\nabout \"x\"\r\nversion 1\r\n");
    }

    #[test]
    fn a_byte_order_mark_stays_first() {
        let original = "\u{feff}about \"x\"\n";
        let new = with_name_line(Some(original.as_bytes()), "Foo").unwrap();
        assert_eq!(
            String::from_utf8(new).unwrap(),
            "\u{feff}name \"Foo\"\nabout \"x\"\n"
        );
    }

    #[test]
    fn a_file_without_a_trailing_newline_is_kept_as_it_is() {
        let new = with_name_line(Some(b"about \"x\""), "Foo").unwrap();
        assert_eq!(new, b"name \"Foo\"\nabout \"x\"");
        // An empty file.
        assert_eq!(with_name_line(Some(b""), "Foo").unwrap(), b"name \"Foo\"\n");
    }

    #[test]
    fn a_file_starting_with_an_indented_line_gets_the_line_at_the_end_instead() {
        // On top, `name` would swallow the indented line as its child.
        let original = "# notes\n\tabout \"x\"\nversion 1";
        let new = with_name_line(Some(original.as_bytes()), "Foo").unwrap();
        assert_eq!(
            String::from_utf8(new.clone()).unwrap(),
            format!("{original}\nname \"Foo\"\n")
        );
        let meta = PluginMeta::parse(std::str::from_utf8(&new).unwrap());
        assert_eq!(meta.name.as_deref(), Some("Foo"));
        assert_eq!(meta.about, "x\n");
    }

    #[test]
    fn a_name_that_does_not_read_back_puts_the_original_file_back() {
        let w = World::new();
        w.add("Foo", Some("about \"x\"\n"));
        // A filesystem that silently mangles what is written.
        let mangle: WriteFn = &|path, _| fs::write(path, "name \"Something else\"\n");
        let err =
            write_name_with(&w.dir("Foo"), Some(b"about \"x\"\n"), "Foo", mangle).unwrap_err();
        assert!(matches!(err, NamingError::ReadBackFailed(_)), "{err:?}");
        assert_eq!(w.plugin_txt("Foo").unwrap(), b"about \"x\"\n");

        // With no file to begin with, the created one is removed again.
        w.add("Bar", None);
        let err = write_name_with(&w.dir("Bar"), None, "Bar", mangle).unwrap_err();
        assert!(matches!(err, NamingError::ReadBackFailed(_)));
        assert_eq!(w.plugin_txt("Bar"), None);
    }

    // --- who can be named ------------------------------------------------

    #[test]
    fn plugins_with_a_name_or_without_a_record_or_with_a_blank_name_line_are_refused() {
        let w = World::new();
        w.add("Named", Some("name \"Has One\"\n"));
        w.add_unlinked("Loose", None);
        w.add("Blank", Some("name \"\"\nabout \"x\"\n"));
        w.add("Spaces", Some("name \"   \"\n"));
        w.add("Bare", Some("about \"x\"\nname\n"));

        assert!(matches!(
            w.plan("Has One", NameChoice::FolderName),
            Err(NamingError::Missing(_))
        ));
        assert!(matches!(
            w.plan("Named", NameChoice::FolderName),
            Err(NamingError::AlreadyNamed { .. })
        ));
        assert!(matches!(
            w.plan("Loose", NameChoice::FolderName),
            Err(NamingError::NotLinked(_))
        ));
        for folder in ["Blank", "Spaces", "Bare"] {
            let err = w.plan(folder, NameChoice::FolderName).unwrap_err();
            assert!(
                matches!(err, NamingError::BlankNameLine(_)),
                "{folder}: {err:?}"
            );
            assert!(err.to_string().contains("by hand"));
        }
        assert!(matches!(
            w.plan("Gone", NameChoice::FolderName),
            Err(NamingError::Missing(_))
        ));
    }

    // --- the folder-name default -----------------------------------------

    #[test]
    fn naming_with_the_folder_name_writes_one_line_and_touches_nothing_else() {
        let w = World::new();
        let original = "about \"An old plugin\"\r\nversion 3\r\n";
        w.add("Foo", Some(original));
        w.add("Other", Some("name \"Other\"\n"));
        w.set_states(&[("Foo", false), ("Other", true)]);
        let mut store = ProfileStore::default();
        store
            .insert("Main", profile(&[("Foo", Some("Foo")), ("Other", None)]))
            .unwrap();
        store.save(&w.profiles_path).unwrap();
        let states_before = fs::read(w.config.join(game_state::PLUGINS_FILE)).unwrap();
        let profiles_before = fs::read(&w.profiles_path).unwrap();

        let plan = w.plan("Foo", NameChoice::FolderName).unwrap();
        assert_eq!(plan.name, "Foo");
        assert_eq!(plan.line, "name \"Foo\"");
        assert!(!plan.identity_changes());
        assert!(!plan.creates_file());
        assert!(!plan.enabled);
        assert!(plan.profiles.is_empty() && plan.dependents.is_empty());
        let outcome = commit_name(&w.paths(), &plan, GameProcess::NotRunning).unwrap();

        assert_eq!(
            w.plugin_txt("Foo").unwrap(),
            format!("name \"Foo\"\r\n{original}").as_bytes()
        );
        // The backup holds the old file, outside the plugin folder.
        assert_eq!(outcome.backup_path, plan.backup_path);
        assert!(
            outcome
                .backup_path
                .starts_with(w.records_path.parent().unwrap())
        );
        assert_eq!(fs::read(&outcome.backup_path).unwrap(), original.as_bytes());
        assert_eq!(outcome.plugin_states_backup, None);
        // plugins.txt and profiles are byte for byte the same.
        assert_eq!(
            fs::read(w.config.join(game_state::PLUGINS_FILE)).unwrap(),
            states_before
        );
        assert_eq!(fs::read(&w.profiles_path).unwrap(), profiles_before);
        // The record remembers the name; the identity is unchanged.
        let record = &w.records()["Foo"];
        assert_eq!(record.identity, "Foo");
        assert_eq!(record.added_name.as_deref(), Some("Foo"));
        assert!(!w.journal_exists());
        // Nothing else appeared in the plugin folder.
        let mut inside: Vec<String> = fs::read_dir(w.dir("Foo"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        inside.sort();
        assert_eq!(inside, ["data", "plugin.txt"]);
        // And the plugin is now named.
        assert_eq!(
            w.installed()
                .iter()
                .find(|p| p.folder == "Foo")
                .unwrap()
                .meta
                .name
                .as_deref(),
            Some("Foo")
        );
    }

    #[test]
    fn a_plugin_without_a_plugin_txt_gets_one_and_a_note_is_kept() {
        let w = World::new();
        w.add("Foo", None);
        let plan = w.plan("Foo", NameChoice::FolderName).unwrap();
        assert!(plan.creates_file());
        let outcome = commit_name(&w.paths(), &plan, GameProcess::NotRunning).unwrap();
        assert_eq!(w.plugin_txt("Foo").unwrap(), b"name \"Foo\"\n");
        assert!(
            outcome
                .backup_path
                .to_string_lossy()
                .ends_with("Foo.no-plugin-txt")
        );
        assert!(
            fs::read_to_string(&outcome.backup_path)
                .unwrap()
                .contains("no plugin.txt")
        );
    }

    #[test]
    fn naming_twice_is_refused_the_second_time() {
        let w = World::new();
        w.add("Foo", None);
        let plan = w.plan("Foo", NameChoice::FolderName).unwrap();
        commit_name(&w.paths(), &plan, GameProcess::NotRunning).unwrap();
        // The same (now stale) plan again: the file changed, nothing is written twice.
        let err = commit_name(&w.paths(), &plan, GameProcess::NotRunning).unwrap_err();
        assert!(matches!(err, NamingError::Changed(_)), "{err:?}");
        assert_eq!(w.plugin_txt("Foo").unwrap(), b"name \"Foo\"\n");
        assert!(matches!(
            w.plan("Foo", NameChoice::FolderName),
            Err(NamingError::AlreadyNamed { .. })
        ));
    }

    // --- refusals at commit time -----------------------------------------

    #[test]
    fn nothing_is_written_while_the_game_runs() {
        let w = World::new();
        w.add("Foo", Some("about \"x\"\n"));
        let plan = w.plan("Foo", typed("New")).unwrap();
        let err = commit_name(&w.paths(), &plan, GameProcess::Running).unwrap_err();
        assert!(matches!(err, NamingError::GameRunning));
        assert_eq!(w.plugin_txt("Foo").unwrap(), b"about \"x\"\n");
        assert!(!w.journal_exists());
        assert!(!backup_dir(&w.records_path).exists());
        // An unknown process list proceeds, like plugins.txt writes do.
        let outcome = commit_name(&w.paths(), &plan, GameProcess::Unknown).unwrap();
        assert!(outcome.game_detection_failed);
    }

    #[test]
    fn a_plan_whose_file_or_folder_changed_meanwhile_is_refused() {
        let w = World::new();
        w.add("Foo", Some("about \"x\"\n"));
        let plan = w.plan("Foo", NameChoice::FolderName).unwrap();
        fs::write(w.dir("Foo").join(PLUGIN_TXT), "about \"edited\"\n").unwrap();
        assert!(matches!(
            commit_name(&w.paths(), &plan, GameProcess::NotRunning),
            Err(NamingError::Changed(_))
        ));
        assert_eq!(w.plugin_txt("Foo").unwrap(), b"about \"edited\"\n");

        let plan = w.plan("Foo", NameChoice::FolderName).unwrap();
        fs::rename(w.dir("Foo"), w.dir("Moved")).unwrap();
        assert!(matches!(
            commit_name(&w.paths(), &plan, GameProcess::NotRunning),
            Err(NamingError::Missing(_))
        ));
    }

    #[test]
    fn an_unfinished_load_order_run_or_naming_blocks_a_new_one() {
        let w = World::new();
        w.add("Foo", None);
        let plan = w.plan("Foo", NameChoice::FolderName).unwrap();
        let journal = w.records_path.with_file_name(LOAD_ORDER_JOURNAL_FILE);
        fs::create_dir_all(journal.parent().unwrap()).unwrap();
        fs::write(&journal, "{}").unwrap();
        assert!(matches!(
            commit_name(&w.paths(), &plan, GameProcess::NotRunning),
            Err(NamingError::Busy)
        ));
        fs::remove_file(&journal).unwrap();
        fs::write(w.paths().journal_path(), "{}").unwrap();
        assert!(matches!(
            commit_name(&w.paths(), &plan, GameProcess::NotRunning),
            Err(NamingError::Busy)
        ));
        assert_eq!(w.plugin_txt("Foo"), None);
    }

    // --- a typed name: identity migration ---------------------------------

    /// Foo (no name) is switched off; two profiles list it; Dep requires it, Opt lists it as
    /// optional, Rival conflicts with it.
    fn migration_world() -> World {
        let w = World::new();
        w.add("Foo", Some("about \"x\"\n"));
        w.add(
            "Dep",
            Some("name \"Dep\"\ndependencies\n\trequires\n\t\tFoo\n"),
        );
        w.add(
            "Opt",
            Some("name \"Opt\"\ndependencies\n\toptional\n\t\tFoo\n"),
        );
        w.add_unlinked(
            "Rival",
            Some("name \"Rival\"\ndependencies\n\tconflicts\n\t\tFoo\n"),
        );
        w.set_states(&[("Foo", false), ("Dep", true)]);
        let mut store = ProfileStore::default();
        store
            .insert("Main", profile(&[("Foo", Some("Foo-Cat")), ("Dep", None)]))
            .unwrap();
        store.insert("Other", profile(&[("Dep", None)])).unwrap();
        store.insert("Third", profile(&[("Foo", None)])).unwrap();
        store.active = Some("Main".into());
        store.save(&w.profiles_path).unwrap();
        w
    }

    #[test]
    fn a_typed_name_moves_the_on_off_entry_profiles_and_record_and_a_disabled_plugin_stays_off() {
        let w = migration_world();
        let plan = w.plan("Foo", typed("  Foo Expanded  ")).unwrap();
        assert_eq!(
            plan.name, "Foo Expanded",
            "the review shows the trimmed name"
        );
        assert!(plan.identity_changes());
        assert!(!plan.enabled);
        assert_eq!(plan.profiles, ["Main", "Third"]);
        let relations: Vec<(&str, Relation)> = plan
            .dependents
            .iter()
            .map(|d| (d.plugin.as_str(), d.relation))
            .collect();
        assert_eq!(
            relations,
            [
                ("Dep", Relation::Requires),
                ("Opt", Relation::Optional),
                ("Rival", Relation::Conflicts)
            ]
        );

        let outcome = commit_name(&w.paths(), &plan, GameProcess::NotRunning).unwrap();
        assert!(outcome.plugin_states_backup.is_some());
        assert_eq!(
            w.plugin_txt("Foo").unwrap(),
            b"name \"Foo Expanded\"\nabout \"x\"\n"
        );
        // Off before, off after: under the new name, and the old entry is gone.
        let states = w.states();
        assert_eq!(states.get("Foo Expanded"), Some(&false));
        assert_eq!(states.get("Foo"), None);
        assert_eq!(states.get("Dep"), Some(&true));
        // Profiles follow the new name and keep the catalog name.
        let store = w.profiles();
        assert_eq!(
            store.profiles["Main"],
            profile(&[("Foo Expanded", Some("Foo-Cat")), ("Dep", None)])
        );
        assert_eq!(store.profiles["Other"], profile(&[("Dep", None)]));
        assert_eq!(store.profiles["Third"], profile(&[("Foo Expanded", None)]));
        assert_eq!(store.active.as_deref(), Some("Main"));
        // The record follows too.
        let record = &w.records()["Foo"];
        assert_eq!(record.identity, "Foo Expanded");
        assert_eq!(record.added_name.as_deref(), Some("Foo Expanded"));
        assert_eq!(record.folder, "Foo", "the folder itself is not renamed");
        // The game now sees the new name.
        let foo = w
            .installed()
            .into_iter()
            .find(|p| p.folder == "Foo")
            .unwrap();
        assert_eq!(foo.identity, "Foo Expanded");
        assert!(!w.journal_exists());
    }

    #[test]
    fn an_enabled_plugin_that_was_not_listed_stays_enabled_under_its_new_name() {
        let w = World::new();
        w.add("Foo", None);
        let plan = w.plan("Foo", typed("Bar")).unwrap();
        assert!(plan.enabled);
        commit_name(&w.paths(), &plan, GameProcess::NotRunning).unwrap();
        assert_eq!(w.states().get("Bar"), Some(&true));
        assert!(game_state::effective_enabled(&w.states(), "Bar"));
    }

    #[test]
    fn a_typed_name_that_is_another_plugins_identity_is_refused() {
        let w = migration_world();
        let err = w.plan("Foo", typed("Dep")).unwrap_err();
        assert!(
            matches!(err, NamingError::Name(NameError::Taken { ref folder }) if folder == "Dep"),
            "{err:?}"
        );
        // Also a plugin the manager doesn't manage.
        assert!(w.plan("Foo", typed("Rival")).is_err());
    }

    #[test]
    fn typing_the_folder_name_is_the_same_as_the_default() {
        let w = migration_world();
        let plan = w.plan("Foo", typed(" Foo ")).unwrap();
        assert!(!plan.identity_changes());
        assert!(plan.profiles.is_empty() && plan.dependents.is_empty());
        let states_before = w.states();
        commit_name(&w.paths(), &plan, GameProcess::NotRunning).unwrap();
        assert_eq!(w.states(), states_before);
    }

    #[test]
    fn when_another_plugin_also_goes_by_the_old_name_its_entries_are_left_for_it() {
        let w = World::new();
        w.add("Foo", None);
        w.add_unlinked("Twin", Some("name \"Foo\"\n"));
        w.set_states(&[("Foo", true)]);
        let mut store = ProfileStore::default();
        store.insert("Main", profile(&[("Foo", None)])).unwrap();
        store.save(&w.profiles_path).unwrap();

        w.name("Foo", typed("Foo Mine"));
        let states = w.states();
        assert_eq!(states.get("Foo"), Some(&true), "still Twin's entry");
        assert_eq!(states.get("Foo Mine"), Some(&true));
        assert_eq!(
            w.profiles().profiles["Main"],
            profile(&[("Foo", None), ("Foo Mine", None)])
        );
    }

    // --- crashes between steps -------------------------------------------

    /// Runs a commit whose name write fails the given way, as a crash would stop it.
    fn crash_at_write(w: &World, plan: &NamingPlan) {
        let fail: WriteFn = &|_, _| Err(io::Error::other("power cut"));
        let err = commit_name_with(&w.paths(), plan, GameProcess::NotRunning, fail).unwrap_err();
        assert!(matches!(err, NamingError::Io(_)), "{err:?}");
    }

    #[test]
    fn a_failed_name_write_undoes_the_new_entry_and_leaves_the_plugin_as_it_was() {
        let w = migration_world();
        w.set_states(&[("Foo", false), ("Foo Expanded", true), ("Dep", true)]);
        let plan = w.plan("Foo", typed("Foo Expanded")).unwrap();
        crash_at_write(&w, &plan);
        assert_eq!(w.plugin_txt("Foo").unwrap(), b"about \"x\"\n");
        // The entry the new name had before is back exactly as it was.
        let states = w.states();
        assert_eq!(states.get("Foo"), Some(&false));
        assert_eq!(states.get("Foo Expanded"), Some(&true));
        assert!(!w.journal_exists());
        assert_eq!(w.records()["Foo"].added_name, None);
    }

    /// Leaves the world as a crash right after the name was written would: the journal and
    /// step 1 on disk, step 3 not done.
    fn crash_after_write(w: &World, plan: &NamingPlan) {
        let mut states = w.states();
        let journal = Journal {
            format: 1,
            folder: plan.folder.clone(),
            name: plan.name.clone(),
            old_identity: plan.old_identity.clone(),
            new_entry_before: states.get(&plan.name).copied(),
        };
        write_journal(&w.paths().journal_path(), &journal).unwrap();
        states.insert(
            plan.name.clone(),
            game_state::effective_enabled(&states, &plan.old_identity),
        );
        game_state::write_plugin_states(&w.config, &states, GameProcess::NotRunning).unwrap();
        let original = w.plugin_txt(&plan.folder);
        write_name(&w.dir(&plan.folder), original.as_deref(), &plan.name).unwrap();
    }

    #[test]
    fn after_a_crash_once_the_name_is_written_recovery_finishes_and_a_disabled_plugin_stays_off() {
        let w = migration_world();
        let plan = w.plan("Foo", typed("Foo Expanded")).unwrap();
        crash_after_write(&w, &plan);
        // Already now, before recovery, the name the game sees is off.
        assert_eq!(w.states().get("Foo Expanded"), Some(&false));

        let recovery = recover(&w.paths(), GameProcess::NotRunning).unwrap();
        assert_eq!(
            recovery,
            NamingRecovery::Finished {
                folder: "Foo".into(),
                name: "Foo Expanded".into()
            }
        );
        assert!(!w.journal_exists());
        assert_eq!(w.states().get("Foo"), None);
        assert_eq!(w.states().get("Foo Expanded"), Some(&false));
        assert_eq!(
            w.records()["Foo"].added_name.as_deref(),
            Some("Foo Expanded")
        );
        assert!(
            w.profiles().profiles["Main"]
                .enabled
                .contains_key("Foo Expanded")
        );
        // Running it again changes nothing.
        let states = w.states();
        let profiles = w.profiles();
        assert_eq!(
            recover(&w.paths(), GameProcess::NotRunning).unwrap(),
            NamingRecovery::Nothing
        );
        // And finishing twice (a crash inside step 3) is harmless too.
        let journal = Journal {
            format: 1,
            folder: "Foo".into(),
            name: "Foo Expanded".into(),
            old_identity: "Foo".into(),
            new_entry_before: None,
        };
        finish(&w.paths(), &journal, GameProcess::NotRunning).unwrap();
        assert_eq!(w.states(), states);
        assert_eq!(w.profiles(), profiles);
    }

    #[test]
    fn after_a_crash_before_the_name_was_written_recovery_undoes_the_new_entry() {
        let w = migration_world();
        let plan = w.plan("Foo", typed("Foo Expanded")).unwrap();
        // Journal and step 1 on disk, the name not written.
        let mut states = w.states();
        write_journal(
            &w.paths().journal_path(),
            &Journal {
                format: 1,
                folder: "Foo".into(),
                name: plan.name.clone(),
                old_identity: "Foo".into(),
                new_entry_before: None,
            },
        )
        .unwrap();
        states.insert(plan.name.clone(), false);
        game_state::write_plugin_states(&w.config, &states, GameProcess::NotRunning).unwrap();

        let recovery = recover(&w.paths(), GameProcess::NotRunning).unwrap();
        assert_eq!(
            recovery,
            NamingRecovery::RolledBack {
                folder: "Foo".into()
            }
        );
        assert_eq!(w.states().get("Foo Expanded"), None);
        assert_eq!(w.states().get("Foo"), Some(&false));
        assert_eq!(w.plugin_txt("Foo").unwrap(), b"about \"x\"\n");
        assert!(!w.journal_exists());
        // The plugin can be named again afterwards.
        w.name("Foo", typed("Foo Expanded"));
        assert_eq!(w.states().get("Foo Expanded"), Some(&false));
    }

    #[test]
    fn recovery_waits_while_the_game_runs_and_refuses_a_journal_it_cannot_trust() {
        let w = migration_world();
        let plan = w.plan("Foo", typed("Foo Expanded")).unwrap();
        crash_after_write(&w, &plan);
        assert!(matches!(
            recover(&w.paths(), GameProcess::Running),
            Err(NamingError::GameRunning)
        ));
        assert!(w.journal_exists(), "left for later");
        // A new naming is refused until it is done.
        w.add("Bar", None);
        let other = w.plan("Bar", NameChoice::FolderName).unwrap();
        assert!(matches!(
            commit_name(&w.paths(), &other, GameProcess::NotRunning),
            Err(NamingError::Busy)
        ));

        for bad in [
            "not json",
            r#"{"format":2,"folder":"Foo","name":"x","old_identity":"Foo","new_entry_before":null}"#,
            r#"{"format":1,"folder":"../outside","name":"x","old_identity":"y","new_entry_before":null}"#,
        ] {
            fs::write(w.paths().journal_path(), bad).unwrap();
            assert!(
                recover(&w.paths(), GameProcess::NotRunning).is_err(),
                "{bad}"
            );
            assert!(w.journal_exists());
        }
    }

    // --- updates ---------------------------------------------------------

    fn record_with_name(folder: &str, original: Option<&str>, name: Option<&str>) -> InstallRecord {
        InstallRecord {
            catalog_name: "Foo-Cat".into(),
            folder: folder.into(),
            identity: name.unwrap_or(folder).into(),
            version: "1".into(),
            source_url: "https://example.com/x.zip".into(),
            sha256: "hash".into(),
            original_folder: original.map(str::to_string),
            added_name: name.map(str::to_string),
        }
    }

    #[test]
    fn an_update_copy_without_a_name_gets_the_added_name_back() {
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join("staged");
        fs::create_dir_all(staged.join("data")).unwrap();
        fs::write(staged.join(PLUGIN_TXT), "about \"v2\"\n").unwrap();
        let backups = dir.path().join("backups");
        let record = record_with_name("020-Foo", Some("Foo"), Some("Foo Expanded"));

        let meta = PluginMeta::parse("about \"v2\"\n");
        assert_eq!(
            name_to_reapply(Some(&record), &meta).as_deref(),
            Some("Foo Expanded")
        );
        let added = reapply_to_staged(&staged, Some(&record), &backups).unwrap();
        assert_eq!(added.as_deref(), Some("Foo Expanded"));
        assert_eq!(
            fs::read_to_string(staged.join(PLUGIN_TXT)).unwrap(),
            "name \"Foo Expanded\"\nabout \"v2\"\n"
        );
        // The backup is keyed by the original folder name, not the numbered one.
        assert_eq!(
            fs::read_to_string(backups.join("Foo.plugin.txt")).unwrap(),
            "about \"v2\"\n"
        );
    }

    #[test]
    fn an_update_copy_that_names_itself_keeps_its_own_name() {
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join("staged");
        fs::create_dir_all(staged.join("data")).unwrap();
        fs::write(staged.join(PLUGIN_TXT), "name \"Upstream\"\n").unwrap();
        let record = record_with_name("Foo", None, Some("Mine"));
        let meta = PluginMeta::parse("name \"Upstream\"\n");
        assert_eq!(name_to_reapply(Some(&record), &meta), None);
        let added = reapply_to_staged(&staged, Some(&record), dir.path()).unwrap();
        assert_eq!(added, None);
        assert_eq!(
            fs::read_to_string(staged.join(PLUGIN_TXT)).unwrap(),
            "name \"Upstream\"\n"
        );
    }

    #[test]
    fn nothing_is_put_back_for_a_plugin_the_manager_never_named_and_a_blank_line_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join("staged");
        fs::create_dir_all(staged.join("data")).unwrap();
        let plain = record_with_name("Foo", None, None);
        assert_eq!(
            reapply_to_staged(&staged, Some(&plain), dir.path()).unwrap(),
            None
        );
        assert_eq!(reapply_to_staged(&staged, None, dir.path()).unwrap(), None);
        assert!(!staged.join(PLUGIN_TXT).exists());

        fs::write(staged.join(PLUGIN_TXT), "name \"\"\n").unwrap();
        let named = record_with_name("Foo", None, Some("Mine"));
        assert!(reapply_to_staged(&staged, Some(&named), dir.path()).is_err());
        assert_eq!(
            fs::read_to_string(staged.join(PLUGIN_TXT)).unwrap(),
            "name \"\"\n"
        );
    }
}
