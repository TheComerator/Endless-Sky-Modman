//! Reading and safely writing the game's `plugins.txt` (decision F).
//!
//! The game rewrites `plugins.txt` from memory when the user toggles plugins in
//! Preferences, so writing while it runs would be silently undone.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::files;
use crate::plugin_state::{self, PluginStates};

pub const PLUGINS_FILE: &str = "plugins.txt";
pub const BACKUP_FILE: &str = "plugins.txt.esmm-bak";

/// The game's executable name per platform, from its CMakeLists.txt `OUTPUT_NAME`s:
/// Linux (native, Steam, Flatpak, AppImage), Windows (also under Proton/Wine), macOS bundle.
/// All fit in the 15 bytes Linux keeps of a process name, so exact matches suffice.
pub const EXECUTABLE_NAMES: [&str; 3] = ["endless-sky", "Endless Sky.exe", "Endless Sky"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameProcess {
    Running,
    NotRunning,
    /// The process list could not be read.
    Unknown,
}

/// Checks the live process list.
pub fn detect_game_process() -> GameProcess {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

    if !sysinfo::IS_SUPPORTED_SYSTEM {
        return GameProcess::Unknown;
    }
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
    );
    let names = system.processes().values().flat_map(|p| {
        let exe_name = p.exe().and_then(Path::file_name);
        std::iter::once(p.name()).chain(exe_name)
    });
    game_process_from(names)
}

/// Decides from a list of process names; an empty list means the list couldn't be read.
pub fn game_process_from<I, S>(process_names: I) -> GameProcess
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut any = false;
    for name in process_names {
        any = true;
        let name = name.as_ref().to_string_lossy();
        if EXECUTABLE_NAMES
            .iter()
            .any(|exe| name.eq_ignore_ascii_case(exe))
        {
            return GameProcess::Running;
        }
    }
    if any {
        GameProcess::NotRunning
    } else {
        GameProcess::Unknown
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteOutcome {
    /// Where the previous `plugins.txt` was copied, if there was one.
    pub backup: Option<PathBuf>,
    /// The game might be running; the UI should warn that changes may be overwritten.
    pub detection_failed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("Endless Sky is running; close it first, or it will overwrite plugins.txt on exit")]
    GameRunning,
    #[error("failed to write plugins.txt: {0}")]
    Io(#[from] io::Error),
}

/// A missing file means no plugin is listed, so every plugin is enabled.
pub fn read_plugin_states(config_dir: &Path) -> io::Result<PluginStates> {
    match fs::read_to_string(config_dir.join(PLUGINS_FILE)) {
        Ok(text) => Ok(plugin_state::parse(&text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(PluginStates::new()),
        Err(e) => Err(e),
    }
}

pub fn write_plugin_states(
    config_dir: &Path,
    states: &PluginStates,
    game: GameProcess,
) -> Result<WriteOutcome, WriteError> {
    if game == GameProcess::Running {
        return Err(WriteError::GameRunning);
    }
    let path = config_dir.join(PLUGINS_FILE);
    let backup_path = config_dir.join(BACKUP_FILE);
    let backup = match fs::copy(&path, &backup_path) {
        Ok(_) => Some(backup_path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    files::write_atomic(&path, plugin_state::write(states).as_bytes())?;
    Ok(WriteOutcome {
        backup,
        detection_failed: game == GameProcess::Unknown,
    })
}

/// The game's default: a plugin not listed in `plugins.txt` is enabled.
pub fn effective_enabled(states: &PluginStates, identity: &str) -> bool {
    states.get(identity).copied().unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn detects_by_process_name() {
        use GameProcess::*;
        assert_eq!(game_process_from(["bash", "endless-sky"]), Running);
        assert_eq!(game_process_from(["Endless Sky.exe"]), Running);
        assert_eq!(game_process_from(["endless sky.EXE"]), Running);
        assert_eq!(game_process_from(["Endless Sky"]), Running);
        assert_eq!(game_process_from(["bash", "Endless Sky Mod"]), NotRunning);
        assert_eq!(game_process_from(["endless-sky-mm"]), NotRunning);
        assert_eq!(game_process_from(Vec::<String>::new()), Unknown);
    }

    #[test]
    fn real_process_list_is_readable_here() {
        assert_ne!(detect_game_process(), GameProcess::Unknown);
    }

    #[test]
    fn missing_file_reads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_plugin_states(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn refuses_while_game_runs() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(PLUGINS_FILE), "state\n\tA 1\n").unwrap();
        let states = PluginStates::from([("A".into(), false)]);
        assert!(matches!(
            write_plugin_states(dir.path(), &states, GameProcess::Running),
            Err(WriteError::GameRunning)
        ));
        assert_eq!(names(dir.path()), [PLUGINS_FILE]);
        assert_eq!(
            fs::read_to_string(dir.path().join(PLUGINS_FILE)).unwrap(),
            "state\n\tA 1\n"
        );
    }

    #[test]
    fn writes_atomically_with_backup() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("plugins")).unwrap();
        let first = PluginStates::from([("A b".into(), true), ("C".into(), false)]);
        let out = write_plugin_states(dir.path(), &first, GameProcess::NotRunning).unwrap();
        assert_eq!(out.backup, None);
        assert!(!out.detection_failed);
        assert_eq!(
            fs::read_to_string(dir.path().join(PLUGINS_FILE)).unwrap(),
            "state\n\t\"A b\" 1\n\tC 0\n"
        );

        let second = PluginStates::from([("C".into(), true)]);
        let out = write_plugin_states(dir.path(), &second, GameProcess::Unknown).unwrap();
        assert_eq!(out.backup, Some(dir.path().join(BACKUP_FILE)));
        assert!(out.detection_failed);
        assert_eq!(read_plugin_states(dir.path()).unwrap(), second);
        assert_eq!(
            plugin_state::parse(&fs::read_to_string(dir.path().join(BACKUP_FILE)).unwrap()),
            first
        );

        write_plugin_states(dir.path(), &first, GameProcess::NotRunning).unwrap();
        assert_eq!(
            plugin_state::parse(&fs::read_to_string(dir.path().join(BACKUP_FILE)).unwrap()),
            second,
            "backup holds the state from just before the latest write"
        );
        assert_eq!(names(dir.path()), ["plugins", PLUGINS_FILE, BACKUP_FILE]);
        assert!(names(&dir.path().join("plugins")).is_empty());
    }

    #[test]
    fn unlisted_plugins_are_enabled() {
        let states = PluginStates::from([("Off".into(), false), ("On".into(), true)]);
        assert!(!effective_enabled(&states, "Off"));
        assert!(effective_enabled(&states, "On"));
        assert!(effective_enabled(&states, "Unlisted"));
    }
}
