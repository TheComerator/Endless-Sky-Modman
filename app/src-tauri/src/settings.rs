//! The shell's own persisted settings and where per-install state lives on disk.
//!
//! Layout under the manager's app-data dir (never the game's config dir, decisions A/E):
//!
//! ```text
//! <app-data>/settings.json                 selected install + user-added installs
//! <app-data>/installs/<id>/records.json    install records for one game config dir
//! <app-data>/installs/<id>/profiles.json   profiles for one game config dir
//! <app-data>/installs/<id>/config-dir.txt  which config dir <id> is, for humans
//! <app-cache>/catalog/                     catalog + icon cache (decision I), shared
//! ```
//!
//! Records and profiles are keyed by the game's config dir because that's what owns
//! `plugins/` and `plugins.txt`: two installs that share a config dir (a Steam and a native
//! install on Linux) share plugins, so they share records and profiles too, while a Flatpak
//! install's separate config dir gets its own.

use std::io;
use std::path::{Path, PathBuf};

use esmm_core::game_install::{DetectEnv, GameInstall, InstallKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SETTINGS_FILE: &str = "settings.json";

/// An install the user added by hand (decision H's override; e.g. an AppImage, or a game run
/// with `-c <dir>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomInstall {
    /// `None`: the default config dir (an executable-only addition, like an AppImage).
    pub config_dir: Option<PathBuf>,
    pub executable: Option<PathBuf>,
}

impl CustomInstall {
    pub fn to_install(&self, env: Option<&DetectEnv>) -> Option<GameInstall> {
        match (&self.config_dir, &self.executable, env) {
            (Some(dir), exe, _) => Some(GameInstall::custom(dir.clone(), exe.clone())),
            (None, Some(exe), Some(env)) => Some(GameInstall::standalone(env, exe.clone())),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// An [`install_key`]; `None` means "the first detected install".
    #[serde(default)]
    pub selected: Option<String>,
    #[serde(default)]
    pub custom: Vec<CustomInstall>,
}

impl Settings {
    /// A missing file is the defaults. A corrupt one is also treated as the defaults rather
    /// than refusing to start: nothing in it is irreplaceable (it's re-derivable by picking
    /// the install again), unlike records or profiles.
    pub fn load(data_dir: &Path) -> Settings {
        std::fs::read_to_string(data_dir.join(SETTINGS_FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, data_dir: &Path) -> io::Result<()> {
        let mut json = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        json.push(b'\n');
        write_atomic(&data_dir.join(SETTINGS_FILE), &json)
    }
}

/// Stable across runs and unique per (kind, config dir, executable), so a selection survives
/// re-detection even if the detection order changes.
pub fn install_key(install: &GameInstall) -> String {
    let kind = match install.kind {
        InstallKind::Standalone => "standalone",
        InstallKind::Steam => "steam",
        InstallKind::Flatpak => "flatpak",
        InstallKind::Custom => "custom",
    };
    let exe = install
        .executable
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    format!("{kind}|{}|{exe}", install.config_dir.display())
}

/// Paths to the manager's own state for one game config dir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPaths {
    pub config_dir: PathBuf,
    pub records: PathBuf,
    pub profiles: PathBuf,
}

impl InstallPaths {
    pub fn new(data_dir: &Path, config_dir: &Path) -> Self {
        let digest = Sha256::digest(config_dir.as_os_str().as_encoded_bytes());
        let id: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
        let dir = data_dir.join("installs").join(id);
        InstallPaths {
            config_dir: config_dir.to_path_buf(),
            records: dir.join("records.json"),
            profiles: dir.join("profiles.json"),
        }
    }

    /// Leaves a plain-text note of which config dir this state belongs to, purely so a human
    /// browsing app-data can tell the hashed folders apart. Best-effort.
    pub fn write_marker(&self) {
        if let Some(dir) = self.records.parent() {
            let marker = dir.join("config-dir.txt");
            if !marker.exists() {
                let _ = std::fs::create_dir_all(dir);
                let _ = std::fs::write(marker, format!("{}\n", self.config_dir.display()));
            }
        }
    }
}

fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_paths_are_per_config_dir_and_stable() {
        let data = Path::new("/data");
        let a = InstallPaths::new(data, Path::new("/home/u/.local/share/endless-sky"));
        let a_again = InstallPaths::new(data, Path::new("/home/u/.local/share/endless-sky"));
        let b = InstallPaths::new(data, Path::new("/home/u/.var/app/x/data/endless-sky"));
        assert_eq!(a, a_again);
        assert_ne!(a.records, b.records);
        assert_eq!(a.records.parent(), a.profiles.parent());
        assert!(a.records.starts_with("/data/installs"));
    }

    #[test]
    fn settings_round_trip_and_tolerate_corruption() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
        let settings = Settings {
            selected: Some("custom|/x|".into()),
            custom: vec![CustomInstall {
                config_dir: Some("/x".into()),
                executable: None,
            }],
        };
        settings.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()), settings);

        std::fs::write(dir.path().join(SETTINGS_FILE), "{not json").unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
    }

    #[test]
    fn keys_distinguish_installs_sharing_a_config_dir() {
        let dir = PathBuf::from("/cfg");
        let custom = GameInstall::custom(dir.clone(), None);
        let custom_exe = GameInstall::custom(dir, Some("/bin/es".into()));
        assert_ne!(install_key(&custom), install_key(&custom_exe));
    }
}
