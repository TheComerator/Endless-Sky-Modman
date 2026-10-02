//! Finding game installs and their config dirs, and launching them (decision H).
//!
//! The game's config dir is `SDL_GetPrefPath(nullptr, "endless-sky")` (SDL2):
//! `%APPDATA%\endless-sky` on Windows, `$XDG_DATA_HOME/endless-sky` (falling back to
//! `~/.local/share/endless-sky`) on Linux, `~/Library/Application Support/endless-sky`
//! on macOS. Steam runs the same binary with no extra arguments, so a Steam install
//! shares the config dir with a standalone one, except under Proton, where it lives
//! in the Wine prefix. Flatpak sets `XDG_DATA_HOME` to `~/.var/app/<id>/data`. Steam-as-
//! Flatpak is a library source, not a config-dir source: its own manifest overrides
//! `XDG_DATA_HOME` to a different, non-standard path (`FLATPAK_STEAM_APP_ID`'s doc comment),
//! but a native Linux `endless-sky` it finds still resolves the normal native config dir --
//! an *(inference)* that Steam's separate game-launching runtime (Pressure Vessel), unlike
//! Flatpak's own sandboxing of Steam's client data, bind-mounts the real host home for game
//! saves, which wasn't directly confirmed from source but matches both wide community usage
//! and this codebase's existing non-Proton handling (uniform regardless of which library
//! it's in).

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub const STEAM_APP_ID: &str = "404410";
pub const STEAM_URL: &str = "steam://rungameid/404410";
pub const FLATPAK_APP_ID: &str = "io.github.endless_sky.endless_sky";
/// Confirmed 2026-10-02 by reading the Flathub manifest and `steam_wrapper.py`: unlike most
/// Flatpak apps (which get Flatpak's automatic `XDG_DATA_HOME` -> `~/.var/app/<id>/data`
/// redirection, as `FLATPAK_APP_ID` above does), Steam's own manifest sets
/// `FLATPAK_STEAM_XDG_DIRS_PREFIX=~/.var/app/com.valvesoftware.Steam`, which the wrapper
/// joins with its own hardcoded `.local/share` (not `data`) to compute `XDG_DATA_HOME`
/// before Steam starts. Steam's own library then lives at `<that>/Steam`.
pub const FLATPAK_STEAM_APP_ID: &str = "com.valvesoftware.Steam";
/// `installdir` from the app's Steam config; used when the app manifest lacks one.
pub const STEAM_INSTALL_DIR: &str = "Endless Sky";
const PREF_DIR: &str = "endless-sky";
// Generous because a first launch can be slow (e.g. an Intel game binary starting under Rosetta
// on an Apple Silicon Mac); this runs off the UI thread, so waiting costs nothing visible.
const VERSION_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Windows,
    MacOs,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
    Standalone,
    Steam,
    Flatpak,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    Executable {
        program: PathBuf,
        args: Vec<OsString>,
    },
    Steam,
    Flatpak,
    /// The config dir exists but no way to start the game was found.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameInstall {
    pub kind: InstallKind,
    pub config_dir: PathBuf,
    pub executable: Option<PathBuf>,
    pub launch: Launch,
}

impl GameInstall {
    /// A user-chosen config dir; launching passes `-c <config_dir>` to the game.
    pub fn custom(config_dir: PathBuf, executable: Option<PathBuf>) -> Self {
        let launch = match &executable {
            Some(exe) => Launch::Executable {
                program: exe.clone(),
                args: vec!["-c".into(), config_dir.clone().into()],
            },
            None => Launch::Unknown,
        };
        GameInstall {
            kind: InstallKind::Custom,
            config_dir,
            executable,
            launch,
        }
    }

    /// A user-chosen executable (e.g. an AppImage) using the default config dir.
    pub fn standalone(env: &DetectEnv, executable: PathBuf) -> Self {
        GameInstall {
            kind: InstallKind::Standalone,
            config_dir: default_config_dir(env),
            launch: Launch::Executable {
                program: executable.clone(),
                args: Vec::new(),
            },
            executable: Some(executable),
        }
    }

    pub fn plugins_dir(&self) -> PathBuf {
        crate::install::plugins_dir(&self.config_dir)
    }
}

/// Everything detection reads from the machine. Absolute system paths (`/usr`, `/var`,
/// `/Applications`) are looked up under `root`; per-user paths come from `home` and `vars`.
#[derive(Debug, Clone)]
pub struct DetectEnv {
    pub os: Os,
    pub home: PathBuf,
    /// Only `APPDATA`, `XDG_DATA_HOME`, `ProgramFiles` and `ProgramFiles(x86)` are read.
    pub vars: HashMap<String, PathBuf>,
    pub root: PathBuf,
    /// Steam roots known from elsewhere (the Windows registry).
    pub steam_roots: Vec<PathBuf>,
}

impl DetectEnv {
    /// Reads the real environment; `None` if the home dir is unknown.
    pub fn from_system() -> Option<Self> {
        let os = Os::current();
        let home = std::env::home_dir()?;
        let vars = [
            "APPDATA",
            "XDG_DATA_HOME",
            "ProgramFiles",
            "ProgramFiles(x86)",
        ]
        .into_iter()
        .filter_map(|k| Some((k.to_string(), PathBuf::from(std::env::var_os(k)?))))
        .collect();
        let steam_roots = if os == Os::Windows {
            windows_registry_steam_path().into_iter().collect()
        } else {
            Vec::new()
        };
        Some(DetectEnv {
            os,
            home,
            vars,
            root: PathBuf::from("/"),
            steam_roots,
        })
    }

    fn var(&self, key: &str) -> Option<&Path> {
        self.vars
            .get(key)
            .map(PathBuf::as_path)
            .filter(|p| !p.as_os_str().is_empty())
    }

    fn sys(&self, path: &str) -> PathBuf {
        self.root.join(path)
    }

    fn xdg_data_home(&self) -> PathBuf {
        self.var("XDG_DATA_HOME")
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.home.join(".local").join("share"))
    }
}

/// What `SDL_GetPrefPath(nullptr, "endless-sky")` returns for a native or Steam install.
pub fn default_config_dir(env: &DetectEnv) -> PathBuf {
    let base = match env.os {
        Os::Linux => env.xdg_data_home(),
        Os::Windows => env
            .var("APPDATA")
            .map(Path::to_path_buf)
            .unwrap_or_else(|| env.home.join("AppData").join("Roaming")),
        Os::MacOs => env.home.join("Library").join("Application Support"),
    };
    base.join(PREF_DIR)
}

pub fn flatpak_config_dir(env: &DetectEnv) -> PathBuf {
    env.home
        .join(".var")
        .join("app")
        .join(FLATPAK_APP_ID)
        .join("data")
        .join(PREF_DIR)
}

/// All installs found, standalone first, then Steam, then Flatpak. Installs that
/// share a config dir are all kept; see [`group_by_config_dir`].
pub fn detect(env: &DetectEnv) -> Vec<GameInstall> {
    let native_config = default_config_dir(env);
    let mut installs: Vec<GameInstall> = standalone_executables(env)
        .into_iter()
        .filter(|exe| exe.is_file())
        .chain(appimage_executables(env))
        .map(|exe| GameInstall::standalone(env, exe))
        .collect();
    installs.extend(detect_steam(env));
    if native_config.is_dir() && !installs.iter().any(|i| i.config_dir == native_config) {
        installs.push(GameInstall {
            kind: InstallKind::Standalone,
            config_dir: native_config,
            executable: None,
            launch: Launch::Unknown,
        });
    }
    if env.os == Os::Linux {
        let config_dir = flatpak_config_dir(env);
        let app_installed = [
            env.xdg_data_home()
                .join("flatpak")
                .join("app")
                .join(FLATPAK_APP_ID),
            env.sys("var/lib/flatpak/app").join(FLATPAK_APP_ID),
        ]
        .iter()
        .any(|p| p.is_dir());
        if app_installed || config_dir.is_dir() {
            installs.push(GameInstall {
                kind: InstallKind::Flatpak,
                config_dir,
                executable: None,
                launch: Launch::Flatpak,
            });
        }
    }
    installs
}

/// Installs grouped by config dir, in first-seen order. Installs in one group share
/// their plugins and `plugins.txt`.
pub fn group_by_config_dir(installs: &[GameInstall]) -> Vec<(&Path, Vec<&GameInstall>)> {
    let mut groups: Vec<(&Path, Vec<&GameInstall>)> = Vec::new();
    for install in installs {
        match groups
            .iter_mut()
            .find(|(dir, _)| *dir == install.config_dir)
        {
            Some((_, members)) => members.push(install),
            None => groups.push((&install.config_dir, vec![install])),
        }
    }
    groups
}

fn standalone_executables(env: &DetectEnv) -> Vec<PathBuf> {
    match env.os {
        Os::Linux => ["usr/games", "usr/local/games", "usr/bin", "usr/local/bin"]
            .iter()
            .map(|dir| env.sys(dir).join("endless-sky"))
            .collect(),
        Os::Windows => ["ProgramFiles", "ProgramFiles(x86)"]
            .iter()
            .filter_map(|k| env.var(k))
            .map(|dir| dir.join("Endless Sky").join("Endless Sky.exe"))
            .collect(),
        Os::MacOs => [env.sys("Applications"), env.home.join("Applications")]
            .iter()
            .map(|dir| dir.join(mac_bundle_executable()))
            .collect(),
    }
}

/// Folders people usually keep an AppImage in (it has no installer, so there's no standard
/// place). Only looked at one level deep: a full-disk crawl would be slow and surprising.
const APPIMAGE_DIRS: [&str; 6] = [
    "Applications",
    "AppImages",
    "Downloads",
    "Desktop",
    ".local/bin",
    "bin",
];

/// Linux only. An AppImage doesn't sandbox its data, so it uses the same config dir as a
/// native install (`GameInstall::standalone`); finding it just gives the launch button
/// something to run. Matched by name, since the release filename varies by version and
/// architecture: `.AppImage` containing both "endless" and "sky" (any case, any separator).
fn appimage_executables(env: &DetectEnv) -> Vec<PathBuf> {
    if env.os != Os::Linux {
        return Vec::new();
    }
    let mut dirs: Vec<PathBuf> = APPIMAGE_DIRS.iter().map(|d| env.home.join(d)).collect();
    dirs.push(env.sys("opt"));
    let mut found = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|path| {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                name.ends_with(".appimage")
                    && name.contains("endless")
                    && name.contains("sky")
                    && path.is_file()
            })
            .collect();
        names.sort();
        found.extend(names);
    }
    found
}

fn mac_bundle_executable() -> PathBuf {
    ["Endless Sky.app", "Contents", "MacOS", "Endless Sky"]
        .iter()
        .collect()
}

fn steam_roots(env: &DetectEnv) -> Vec<PathBuf> {
    let mut roots = env.steam_roots.clone();
    match env.os {
        Os::Linux => {
            roots.push(env.home.join(".steam").join("steam"));
            roots.push(env.home.join(".steam").join("root"));
            roots.push(env.xdg_data_home().join("Steam"));
            roots.push(
                env.home
                    .join(".var")
                    .join("app")
                    .join(FLATPAK_STEAM_APP_ID)
                    .join(".local")
                    .join("share")
                    .join("Steam"),
            );
        }
        Os::Windows => {
            for k in ["ProgramFiles(x86)", "ProgramFiles"] {
                if let Some(dir) = env.var(k) {
                    roots.push(dir.join("Steam"));
                }
            }
        }
        Os::MacOs => roots.push(
            env.home
                .join("Library")
                .join("Application Support")
                .join("Steam"),
        ),
    }
    roots
}

/// Every Steam library on the machine (each root plus the ones it lists), deduplicated.
pub fn steam_libraries(env: &DetectEnv) -> Vec<PathBuf> {
    let mut libraries: Vec<PathBuf> = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    for root in steam_roots(env) {
        if !root.join("steamapps").is_dir() {
            continue;
        }
        let listed = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf"))
            .map(|text| parse_library_folders(&text))
            .unwrap_or_default();
        for library in std::iter::once(root).chain(listed) {
            let key = library.canonicalize().unwrap_or_else(|_| library.clone());
            if library.join("steamapps").is_dir() && !seen.contains(&key) {
                seen.push(key);
                libraries.push(library);
            }
        }
    }
    libraries
}

fn detect_steam(env: &DetectEnv) -> Vec<GameInstall> {
    let mut installs = Vec::new();
    for library in steam_libraries(env) {
        let steamapps = library.join("steamapps");
        let Ok(manifest) =
            std::fs::read_to_string(steamapps.join(format!("appmanifest_{STEAM_APP_ID}.acf")))
        else {
            continue;
        };
        let install_dir = parse_vdf(&manifest)
            .get("AppState")
            .and_then(|s| s.get("installdir"))
            .and_then(Vdf::as_str)
            .unwrap_or(STEAM_INSTALL_DIR)
            .to_string();
        let game_dir = steamapps.join("common").join(install_dir);
        let proton_config = steamapps
            .join("compatdata")
            .join(STEAM_APP_ID)
            .join("pfx")
            .join("drive_c")
            .join("users")
            .join("steamuser")
            .join("AppData")
            .join("Roaming")
            .join(PREF_DIR);
        let windows_exe = game_dir.join("Endless Sky.exe");
        let (config_dir, exe) = match env.os {
            Os::Linux => {
                let native_exe = game_dir.join("endless-sky");
                let proton =
                    !native_exe.is_file() && (windows_exe.is_file() || proton_config.is_dir());
                if proton {
                    (proton_config, windows_exe)
                } else {
                    (default_config_dir(env), native_exe)
                }
            }
            Os::Windows => (default_config_dir(env), windows_exe),
            Os::MacOs => (
                default_config_dir(env),
                game_dir.join(mac_bundle_executable()),
            ),
        };
        installs.push(GameInstall {
            kind: InstallKind::Steam,
            config_dir,
            executable: exe.is_file().then_some(exe),
            launch: Launch::Steam,
        });
    }
    installs
}

/// Library paths from Steam's `steamapps/libraryfolders.vdf`, in file order. Handles
/// both the current format (`"0" { "path" "..." }`) and the old one (`"1" "D:\\Steam"`).
pub fn parse_library_folders(text: &str) -> Vec<PathBuf> {
    let root = parse_vdf(text);
    let Some(Vdf::Block(entries)) = root.get("libraryfolders") else {
        return Vec::new();
    };
    entries
        .iter()
        .filter(|(key, _)| key.parse::<u32>().is_ok())
        .filter_map(|(_, value)| match value {
            Vdf::Str(path) => Some(path.as_str()),
            Vdf::Block(_) => value.get("path").and_then(Vdf::as_str),
        })
        .map(PathBuf::from)
        .collect()
}

#[derive(Debug)]
enum Vdf {
    Str(String),
    Block(Vec<(String, Vdf)>),
}

impl Vdf {
    fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Block(entries) => entries
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v),
            Vdf::Str(_) => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Vdf::Str(s) => Some(s),
            Vdf::Block(_) => None,
        }
    }
}

enum Token {
    Open,
    Close,
    Text(String),
}

/// Valve KeyValues text: quoted or bare tokens, `{` `}` blocks, `//` comments.
/// Lenient: malformed input yields whatever parsed before the problem.
fn parse_vdf(text: &str) -> Vdf {
    let mut tokens = tokenize_vdf(text).into_iter();
    Vdf::Block(parse_vdf_block(&mut tokens))
}

fn parse_vdf_block(tokens: &mut impl Iterator<Item = Token>) -> Vec<(String, Vdf)> {
    let mut entries = Vec::new();
    while let Some(Token::Text(key)) = tokens.next() {
        match tokens.next() {
            Some(Token::Text(value)) => entries.push((key, Vdf::Str(value))),
            Some(Token::Open) => entries.push((key, Vdf::Block(parse_vdf_block(tokens)))),
            _ => break,
        }
    }
    entries
}

fn tokenize_vdf(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => tokens.push(Token::Open),
            '}' => tokens.push(Token::Close),
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(other) => s.push(other),
                            None => break,
                        },
                        _ => s.push(c),
                    }
                }
                tokens.push(Token::Text(s));
            }
            c if c.is_whitespace() => {}
            c => {
                let mut s = String::from(c);
                while let Some(&next) = chars.peek() {
                    if next.is_whitespace() || matches!(next, '{' | '}' | '"') {
                        break;
                    }
                    s.push(next);
                    chars.next();
                }
                tokens.push(Token::Text(s));
            }
        }
    }
    tokens
}

/// `SteamPath` from `reg query HKCU\Software\Valve\Steam /v SteamPath` output.
pub fn parse_reg_query(output: &str) -> Option<PathBuf> {
    output.lines().find_map(|line| {
        let (_, value) = line.split_once("REG_SZ")?;
        let value = value.trim();
        (!value.is_empty()).then(|| PathBuf::from(value))
    })
}

fn windows_registry_steam_path() -> Option<PathBuf> {
    let output = Command::new("reg")
        .args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    parse_reg_query(&String::from_utf8_lossy(&output.stdout))
}

/// The version from `endless-sky --version`, which prints `Endless Sky ver. 0.11.3.0`
/// (`-alpha` suffixed for unreleased builds; three numbers before 0.11) to stderr.
pub fn parse_version_output(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let version = line.trim().strip_prefix("Endless Sky ver.")?.trim();
        (!version.is_empty()).then(|| version.to_string())
    })
}

/// Runs `<executable> --version` with a short timeout. Any failure gives `None`.
pub fn query_version(executable: &Path) -> Option<String> {
    let mut command = Command::new(executable);
    command.arg("--version");
    run_for_version(command)
}

/// Like [`query_version`], but also handles Flatpak installs.
pub fn query_install_version(install: &GameInstall) -> Option<String> {
    match (&install.launch, &install.executable) {
        (Launch::Flatpak, _) => {
            let mut command = Command::new("flatpak");
            command.args(["run", FLATPAK_APP_ID, "--version"]);
            run_for_version(command)
        }
        (_, Some(exe)) => query_version(exe),
        (_, None) => {
            tracing::warn!(
                "no game executable known for this install, so its version can't be read"
            );
            None
        }
    }
}

fn run_for_version(mut command: Command) -> Option<String> {
    let mut child = match command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            tracing::warn!(error = %e, program = ?command.get_program(), "couldn't run the game to read its version");
            return None;
        }
    };
    let stdout = child.stdout.take().map(drain);
    let stderr = child.stderr.take().map(drain);
    let deadline = Instant::now() + VERSION_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            _ => {
                tracing::warn!(timeout = ?VERSION_TIMEOUT, "game didn't answer --version in time");
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let output: String = [stderr, stdout]
        .into_iter()
        .flatten()
        .filter_map(|h| h.join().ok())
        .collect();
    let version = parse_version_output(&output);
    match &version {
        Some(v) => tracing::info!(version = %v, "read the game's version"),
        None => tracing::warn!(
            output = %output.chars().take(300).collect::<String>(),
            "game ran but its --version output wasn't recognised"
        ),
    }
    version
}

fn drain(mut pipe: impl Read + Send + 'static) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// The command that starts this install, built without running anything.
/// `None` for [`Launch::Unknown`].
pub fn launch_command(install: &GameInstall, os: Os) -> Option<Command> {
    let command = match &install.launch {
        Launch::Executable { program, args } => {
            let mut c = Command::new(program);
            c.args(args);
            c
        }
        Launch::Steam => {
            let mut c;
            match os {
                Os::Linux => {
                    c = Command::new("steam");
                    c.arg(STEAM_URL);
                }
                Os::Windows => {
                    c = Command::new("cmd");
                    c.args(["/C", "start", "", STEAM_URL]);
                }
                Os::MacOs => {
                    c = Command::new("open");
                    c.arg(STEAM_URL);
                }
            }
            c
        }
        Launch::Flatpak => {
            let mut c = Command::new("flatpak");
            c.args(["run", FLATPAK_APP_ID]);
            c
        }
        Launch::Unknown => return None,
    };
    Some(command)
}

/// Starts the game. On Linux a Steam launch falls back to `xdg-open` when the
/// `steam` command isn't on the PATH.
pub fn launch(install: &GameInstall) -> io::Result<Child> {
    tracing::info!(launch = ?install.launch, "launching game");
    let os = Os::current();
    let mut command = launch_command(install, os)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no known way to launch"))?;
    let result = match command.spawn() {
        Err(e)
            if e.kind() == io::ErrorKind::NotFound
                && os == Os::Linux
                && install.launch == Launch::Steam =>
        {
            tracing::info!("steam command not found, falling back to xdg-open");
            Command::new("xdg-open").arg(STEAM_URL).spawn()
        }
        result => result,
    };
    if let Err(e) = &result {
        tracing::error!(error = %e, "failed to launch game");
    }
    result
}
