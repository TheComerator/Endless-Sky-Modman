//! Extracting a plugin zip, finding the plugin inside it, and swapping it into `plugins/`.
//!
//! The game loads every folder in `plugins/` that passes `is_plugin`, so nothing temporary
//! ever goes there: staging and old copies live in `<config>/.esmm-tmp/`, on the same
//! filesystem, so moves in and out of `plugins/` are atomic renames.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use tempfile::TempDir;

use crate::plugin_meta::PluginMeta;

pub const TMP_DIR_NAME: &str = ".esmm-tmp";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractLimits {
    pub max_total_bytes: u64,
    pub max_entries: usize,
}

impl Default for ExtractLimits {
    fn default() -> Self {
        Self {
            max_total_bytes: 2 << 30,
            max_entries: 100_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Extracted {
    pub files: usize,
    pub bytes: u64,
    /// Entries not written: paths escaping the staging dir, and symlinks.
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub path: PathBuf,
    pub folder: String,
    /// Parsed `plugin.txt`, if the plugin has one.
    pub meta: Option<PluginMeta>,
}

impl Installed {
    /// The name the game will use for this plugin.
    pub fn identity(&self) -> &str {
        match &self.meta {
            Some(meta) => meta.identity(&self.folder),
            None => &self.folder,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("invalid zip file: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("zip has more than {0} entries")]
    TooManyEntries(usize),
    #[error("zip expands to more than {0} bytes")]
    TooLarge(u64),
    #[error(
        "download does not contain an Endless Sky plugin (no data/, images/, shaders/ or sounds/ folder)"
    )]
    NotAPlugin,
    #[error("download contains more than one plugin folder: {0:?}")]
    Ambiguous(Vec<PathBuf>),
    #[error("not a safe plugin folder name: {0:?}")]
    InvalidFolderName(String),
    #[error("plugin folder {0:?} is not installed")]
    NotInstalled(String),
}

pub fn plugins_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("plugins")
}

pub fn tmp_dir(config_dir: &Path) -> PathBuf {
    config_dir.join(TMP_DIR_NAME)
}

/// A fresh staging directory inside `tmp_dir`, deleted when dropped.
pub fn new_staging_dir(tmp_dir: &Path) -> io::Result<TempDir> {
    fs::create_dir_all(tmp_dir)?;
    tempfile::Builder::new()
        .prefix("staging-")
        .tempdir_in(tmp_dir)
}

/// The game's `PluginManager::IsPlugin` rule.
pub fn is_plugin(dir: &Path) -> bool {
    ["data", "images", "shaders", "sounds"]
        .iter()
        .any(|sub| dir.join(sub).is_dir())
}

pub fn extract_zip(
    zip_path: &Path,
    staging_dir: &Path,
    limits: ExtractLimits,
) -> Result<Extracted, InstallError> {
    let mut archive = zip::ZipArchive::new(File::open(zip_path)?)?;
    if archive.len() > limits.max_entries {
        return Err(InstallError::TooManyEntries(limits.max_entries));
    }
    fs::create_dir_all(staging_dir)?;
    let mut report = Extracted::default();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let relative = match entry.enclosed_name() {
            // Re-checked in case enclosed_name's rules ever loosen.
            Some(p)
                if !entry.is_symlink()
                    && p.components().all(|c| matches!(c, Component::Normal(_))) =>
            {
                p
            }
            _ => {
                report.skipped.push(entry.name().to_string());
                continue;
            }
        };
        let out_path = staging_dir.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        // Count real bytes, not the header's claimed size, which can lie.
        let remaining = limits.max_total_bytes - report.bytes;
        let mut out = File::create(&out_path)?;
        let written = io::copy(
            &mut (&mut entry).take(remaining.saturating_add(1)),
            &mut out,
        )?;
        if written > remaining {
            return Err(InstallError::TooLarge(limits.max_total_bytes));
        }
        report.bytes += written;
        report.files += 1;
    }
    Ok(report)
}

/// The shallowest folder (staging dir itself, then up to 2 levels down) that the game would load.
pub fn find_plugin_root(staging_dir: &Path) -> Result<PathBuf, InstallError> {
    let mut level = vec![staging_dir.to_path_buf()];
    for _ in 0..=2 {
        let found: Vec<PathBuf> = level.iter().filter(|d| is_plugin(d)).cloned().collect();
        match found.len() {
            0 => {}
            1 => return Ok(found.into_iter().next().unwrap()),
            _ => return Err(InstallError::Ambiguous(found)),
        }
        let mut next = Vec::new();
        for dir in &level {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    next.push(entry.path());
                }
            }
        }
        next.sort();
        level = next;
    }
    Err(InstallError::NotAPlugin)
}

/// A folder name valid on Windows, macOS and Linux.
pub fn sanitize_folder_name(catalog_name: &str) -> String {
    let mut name: String = catalog_name
        .chars()
        .map(|c| {
            if c.is_control() || r#"<>:"/\|?*"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    name.truncate(name.trim_end_matches(['.', ' ']).len());
    if name.is_empty() {
        return "_".to_string();
    }
    // Windows reserves these even with an extension ("con.txt") or trailing spaces before it.
    let stem = name.split('.').next().unwrap_or("").trim_end_matches(' ');
    let reserved = ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|r| stem.eq_ignore_ascii_case(r))
        || (stem.len() == 4
            && stem.is_ascii()
            && (stem[..3].eq_ignore_ascii_case("COM") || stem[..3].eq_ignore_ascii_case("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        name.insert(0, '_');
    }
    name
}

fn check_folder_name(folder_name: &str) -> Result<(), InstallError> {
    if sanitize_folder_name(folder_name) == folder_name {
        Ok(())
    } else {
        Err(InstallError::InvalidFolderName(folder_name.to_string()))
    }
}

/// Move `staged_root` to `plugins_dir/folder_name`, replacing any existing copy.
///
/// `folder_name` must be stable across versions (see `sanitize_folder_name`), so updates keep
/// the plugin's identity and enabled state.
pub fn install(
    staged_root: &Path,
    plugins_dir: &Path,
    tmp_dir: &Path,
    folder_name: &str,
) -> Result<Installed, InstallError> {
    check_folder_name(folder_name)?;
    if !is_plugin(staged_root) {
        return Err(InstallError::NotAPlugin);
    }
    fs::create_dir_all(plugins_dir)?;
    fs::create_dir_all(tmp_dir)?;
    let dest = plugins_dir.join(folder_name);

    let old = if fs::symlink_metadata(&dest).is_ok() {
        let holder = tempfile::Builder::new()
            .prefix("old-")
            .tempdir_in(tmp_dir)?;
        let old_path = holder.path().join(folder_name);
        fs::rename(&dest, &old_path)?;
        Some((holder, old_path))
    } else {
        None
    };
    if let Err(e) = fs::rename(staged_root, &dest) {
        if let Some((_, old_path)) = &old {
            fs::rename(old_path, &dest)?;
        }
        return Err(e.into());
    }
    if let Some((holder, _)) = old {
        holder.close()?;
    }

    let meta = match fs::read_to_string(dest.join("plugin.txt")) {
        Ok(text) => Some(PluginMeta::parse(&text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    Ok(Installed {
        path: dest,
        folder: folder_name.to_string(),
        meta,
    })
}

/// Move the plugin out of `plugins/` first so the game never sees a half-deleted folder.
pub fn uninstall(
    plugins_dir: &Path,
    tmp_dir: &Path,
    folder_name: &str,
) -> Result<(), InstallError> {
    check_folder_name(folder_name)?;
    let dest = plugins_dir.join(folder_name);
    if fs::symlink_metadata(&dest).is_err() {
        return Err(InstallError::NotInstalled(folder_name.to_string()));
    }
    fs::create_dir_all(tmp_dir)?;
    let holder = tempfile::Builder::new()
        .prefix("removed-")
        .tempdir_in(tmp_dir)?;
    fs::rename(&dest, holder.path().join(folder_name))?;
    holder.close()?;
    Ok(())
}
