//! Game install detection against fake homes and roots in temp dirs.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use esmm_core::game_install::{
    DetectEnv, FLATPAK_APP_ID, FLATPAK_STEAM_APP_ID, GameInstall, InstallKind, Launch, Os,
    STEAM_URL, default_config_dir, detect, group_by_config_dir, launch_command,
    parse_bundle_version, parse_library_folders, parse_reg_query, parse_version_output,
    query_version, read_bundle_version,
};
use tempfile::TempDir;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/game-install");

/// Real `--version` output of 0.11.3 (stderr), per `PrintVersion` in main.cpp.
const VERSION_OUTPUT: &str = "\nEndless Sky ver. 0.11.3.0\n\
License GPLv3+: GNU GPL version 3 or later: <https://gnu.org/licenses/gpl.html>\n\
This is free software: you are free to change and redistribute it.\n\
There is NO WARRANTY, to the extent permitted by law.\n\n\
Compiled against SDL v2.30.0\nUsing SDL v2.30.0\n\n";

struct Fake {
    _tmp: TempDir,
    env: DetectEnv,
}

impl Fake {
    fn new(os: Os) -> Self {
        let tmp = TempDir::new().unwrap();
        let env = DetectEnv {
            os,
            home: tmp.path().join("home"),
            vars: HashMap::new(),
            root: tmp.path().join("root"),
            steam_roots: Vec::new(),
        };
        fs::create_dir_all(&env.home).unwrap();
        fs::create_dir_all(&env.root).unwrap();
        Fake { _tmp: tmp, env }
    }

    fn home(&self, rel: &str) -> PathBuf {
        self.env.home.join(rel)
    }

    fn root(&self, rel: &str) -> PathBuf {
        self.env.root.join(rel)
    }

    fn var(&mut self, key: &str, value: &Path) {
        self.env.vars.insert(key.to_string(), value.to_path_buf());
    }
}

fn mkdir(path: &Path) {
    fs::create_dir_all(path).unwrap();
}

fn touch(path: &Path) {
    mkdir(path.parent().unwrap());
    fs::write(path, "").unwrap();
}

fn steam_library(library: &Path, game_files: &[&str]) {
    let steamapps = library.join("steamapps");
    mkdir(&steamapps);
    fs::copy(
        Path::new(FIXTURES).join("appmanifest_404410.acf"),
        steamapps.join("appmanifest_404410.acf"),
    )
    .unwrap();
    for file in game_files {
        touch(&steamapps.join("common/Endless Sky").join(file));
    }
}

fn vdf_string(path: &Path) -> String {
    path.to_str().unwrap().replace('\\', "\\\\")
}

fn write_library_folders(steam_root: &Path, libraries: &[&Path]) {
    let mut text = String::from("\"libraryfolders\"\n{\n");
    for (i, library) in libraries.iter().enumerate() {
        text += &format!(
            "\t\"{i}\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t\t\"label\"\t\t\"\"\n\t\t\"apps\"\n\t\t{{\n\t\t\t\"404410\"\t\t\"412938571\"\n\t\t}}\n\t}}\n",
            vdf_string(library)
        );
    }
    text += "}\n";
    mkdir(&steam_root.join("steamapps"));
    fs::write(steam_root.join("steamapps/libraryfolders.vdf"), text).unwrap();
}

fn kinds(installs: &[GameInstall]) -> Vec<InstallKind> {
    installs.iter().map(|i| i.kind).collect()
}

fn program_and_args(command: &std::process::Command) -> (String, Vec<String>) {
    let program = command.get_program().to_string_lossy().into_owned();
    let args = command
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    (program, args)
}

#[test]
fn empty_home_detects_nothing() {
    for os in [Os::Linux, Os::Windows, Os::MacOs] {
        let fake = Fake::new(os);
        assert!(detect(&fake.env).is_empty(), "{os:?}");
    }
}

#[test]
fn linux_native_config_without_xdg_data_home() {
    let fake = Fake::new(Os::Linux);
    let config = fake.home(".local/share/endless-sky");
    mkdir(&config.join("plugins"));
    assert_eq!(default_config_dir(&fake.env), config);

    let installs = detect(&fake.env);
    assert_eq!(installs.len(), 1);
    assert_eq!(installs[0].kind, InstallKind::Standalone);
    assert_eq!(installs[0].config_dir, config);
    assert_eq!(installs[0].launch, Launch::Unknown);
    assert_eq!(installs[0].plugins_dir(), config.join("plugins"));
    assert!(launch_command(&installs[0], Os::Linux).is_none());
}

#[test]
fn linux_native_respects_xdg_data_home_and_finds_system_executable() {
    let mut fake = Fake::new(Os::Linux);
    let xdg = fake.home("xdg-data");
    fake.var("XDG_DATA_HOME", &xdg);
    mkdir(&fake.home(".local/share/endless-sky"));
    let exe = fake.root("usr/games/endless-sky");
    touch(&exe);

    let installs = detect(&fake.env);
    assert_eq!(installs.len(), 1, "{installs:?}");
    assert_eq!(installs[0].config_dir, xdg.join("endless-sky"));
    assert_eq!(installs[0].executable.as_deref(), Some(exe.as_path()));
    assert_eq!(
        installs[0].launch,
        Launch::Executable {
            program: exe.clone(),
            args: vec![]
        }
    );
}

#[test]
fn empty_xdg_data_home_counts_as_unset() {
    let mut fake = Fake::new(Os::Linux);
    fake.var("XDG_DATA_HOME", Path::new(""));
    assert_eq!(
        default_config_dir(&fake.env),
        fake.home(".local/share/endless-sky")
    );
}

#[test]
fn linux_flatpak_uses_sandboxed_data_dir() {
    let fake = Fake::new(Os::Linux);
    mkdir(&fake.root("var/lib/flatpak/app").join(FLATPAK_APP_ID));

    let installs = detect(&fake.env);
    assert_eq!(kinds(&installs), [InstallKind::Flatpak]);
    assert_eq!(
        installs[0].config_dir,
        fake.home(".var/app/io.github.endless_sky.endless_sky/data/endless-sky")
    );
    let (program, args) = program_and_args(&launch_command(&installs[0], Os::Linux).unwrap());
    assert_eq!(program, "flatpak");
    assert_eq!(args, ["run", FLATPAK_APP_ID]);
}

#[test]
fn linux_flatpak_found_from_config_dir_or_user_install() {
    let fake = Fake::new(Os::Linux);
    mkdir(&fake.home(".var/app/io.github.endless_sky.endless_sky/data/endless-sky"));
    assert_eq!(kinds(&detect(&fake.env)), [InstallKind::Flatpak]);

    let fake = Fake::new(Os::Linux);
    mkdir(&fake.home(".local/share/flatpak/app").join(FLATPAK_APP_ID));
    assert_eq!(kinds(&detect(&fake.env)), [InstallKind::Flatpak]);
}

#[test]
fn linux_steam_with_two_libraries_shares_native_config() {
    let fake = Fake::new(Os::Linux);
    let steam_root = fake.home(".steam/steam");
    let second = fake.home("games/SteamLibrary");
    write_library_folders(&steam_root, &[&steam_root, &second]);
    steam_library(&second, &["endless-sky"]);
    let config = fake.home(".local/share/endless-sky");
    mkdir(&config);
    let standalone = fake.root("usr/bin/endless-sky");
    touch(&standalone);

    let installs = detect(&fake.env);
    assert_eq!(
        kinds(&installs),
        [InstallKind::Standalone, InstallKind::Steam]
    );
    let steam = &installs[1];
    assert_eq!(steam.config_dir, config);
    assert_eq!(
        steam.executable.as_deref(),
        Some(
            second
                .join("steamapps/common/Endless Sky/endless-sky")
                .as_path()
        )
    );
    assert_eq!(steam.launch, Launch::Steam);

    let groups = group_by_config_dir(&installs);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].0, config);
    assert_eq!(groups[0].1.len(), 2);
}

#[test]
fn linux_steam_as_flatpak_library_is_found_and_shares_native_config() {
    let fake = Fake::new(Os::Linux);
    // Steam's own Flatpak manifest overrides XDG_DATA_HOME to this path (confirmed by
    // reading the Flathub manifest and steam_wrapper.py), unlike the generic Flatpak
    // auto-redirect every other sandboxed app gets.
    let steam_root = fake.home(&format!(
        ".var/app/{FLATPAK_STEAM_APP_ID}/.local/share/Steam"
    ));
    write_library_folders(&steam_root, &[&steam_root]);
    steam_library(&steam_root, &["endless-sky"]);
    let config = fake.home(".local/share/endless-sky");
    mkdir(&config);

    let installs = detect(&fake.env);
    assert_eq!(kinds(&installs), [InstallKind::Steam]);
    // A native Linux executable was found, so the existing native-vs-Proton check (which
    // doesn't care which Steam library it came from) resolves the normal native config dir,
    // not anything under the Flatpak sandbox's own data directory.
    assert_eq!(installs[0].config_dir, config);
    assert_eq!(
        installs[0].executable.as_deref(),
        Some(
            steam_root
                .join("steamapps/common/Endless Sky/endless-sky")
                .as_path()
        )
    );
}

#[test]
fn linux_appimage_is_found_by_name_in_the_usual_folders() {
    let fake = Fake::new(Os::Linux);
    let appimage = fake.home("Applications/Endless_Sky-x86_64.AppImage");
    touch(&appimage);
    touch(&fake.home("Applications/Other_Game-x86_64.AppImage"));
    touch(&fake.home("Downloads/endless-sky-notes.txt"));
    let config = fake.home(".local/share/endless-sky");
    mkdir(&config);

    let installs = detect(&fake.env);
    assert_eq!(kinds(&installs), [InstallKind::Standalone]);
    assert_eq!(
        installs[0].config_dir, config,
        "shares the native config dir"
    );
    assert_eq!(installs[0].executable.as_deref(), Some(appimage.as_path()));
    let (program, args) = program_and_args(&launch_command(&installs[0], Os::Linux).unwrap());
    assert_eq!(Path::new(&program), appimage.as_path());
    assert!(args.is_empty());
}

#[test]
fn appimages_are_only_looked_for_on_linux() {
    let fake = Fake::new(Os::MacOs);
    touch(&fake.home("Applications/Endless_Sky-x86_64.AppImage"));
    assert!(detect(&fake.env).is_empty());
}

#[test]
fn linux_steam_root_symlinks_are_not_listed_twice() {
    let fake = Fake::new(Os::Linux);
    let real = fake.home(".local/share/Steam");
    write_library_folders(&real, &[&real]);
    steam_library(&real, &["endless-sky"]);
    mkdir(&fake.home(".steam"));
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, fake.home(".steam/steam")).unwrap();

    assert_eq!(kinds(&detect(&fake.env)), [InstallKind::Steam]);
}

#[test]
fn linux_steam_under_proton_uses_the_wine_prefix() {
    let fake = Fake::new(Os::Linux);
    let steam_root = fake.home(".local/share/Steam");
    write_library_folders(&steam_root, &[&steam_root]);
    steam_library(&steam_root, &["Endless Sky.exe"]);

    let installs = detect(&fake.env);
    assert_eq!(kinds(&installs), [InstallKind::Steam]);
    assert_eq!(
        installs[0].config_dir,
        steam_root.join(
            "steamapps/compatdata/404410/pfx/drive_c/users/steamuser/AppData/Roaming/endless-sky"
        )
    );
}

#[test]
fn steam_library_without_the_game_is_ignored() {
    let fake = Fake::new(Os::Linux);
    let steam_root = fake.home(".steam/steam");
    write_library_folders(&steam_root, &[&steam_root]);
    assert!(detect(&fake.env).is_empty());
}

#[test]
fn windows_appdata_program_files_and_steam() {
    let mut fake = Fake::new(Os::Windows);
    let appdata = fake.home("AppData/Roaming");
    let program_files = fake.root("Program Files");
    let program_files_x86 = fake.root("Program Files (x86)");
    fake.var("APPDATA", &appdata);
    fake.var("ProgramFiles", &program_files);
    fake.var("ProgramFiles(x86)", &program_files_x86);
    let exe = program_files.join("Endless Sky/Endless Sky.exe");
    touch(&exe);
    let steam_root = program_files_x86.join("Steam");
    write_library_folders(&steam_root, &[&steam_root]);
    steam_library(&steam_root, &["Endless Sky.exe"]);

    let installs = detect(&fake.env);
    assert_eq!(
        kinds(&installs),
        [InstallKind::Standalone, InstallKind::Steam]
    );
    for install in &installs {
        assert_eq!(install.config_dir, appdata.join("endless-sky"));
    }
    assert_eq!(installs[0].executable.as_deref(), Some(exe.as_path()));
}

#[test]
fn windows_without_appdata_falls_back_to_roaming() {
    let fake = Fake::new(Os::Windows);
    let config = fake.home("AppData").join("Roaming").join("endless-sky");
    mkdir(&config);
    let installs = detect(&fake.env);
    assert_eq!(kinds(&installs), [InstallKind::Standalone]);
    assert_eq!(installs[0].config_dir, config);
}

#[test]
fn windows_registry_steam_root_is_used() {
    let mut fake = Fake::new(Os::Windows);
    let steam_root = fake.root("Games/Steam");
    steam_library(&steam_root, &["Endless Sky.exe"]);
    fake.env.steam_roots.push(steam_root);
    assert_eq!(kinds(&detect(&fake.env)), [InstallKind::Steam]);
}

#[test]
fn macos_application_support_and_bundle() {
    let fake = Fake::new(Os::MacOs);
    let config = fake.home("Library/Application Support/endless-sky");
    mkdir(&config.join("plugins"));
    let exe = fake.root("Applications/Endless Sky.app/Contents/MacOS/Endless Sky");
    touch(&exe);

    let installs = detect(&fake.env);
    assert_eq!(kinds(&installs), [InstallKind::Standalone]);
    assert_eq!(installs[0].config_dir, config);
    assert_eq!(installs[0].executable.as_deref(), Some(exe.as_path()));
}

#[test]
fn macos_steam() {
    let fake = Fake::new(Os::MacOs);
    let steam_root = fake.home("Library/Application Support/Steam");
    write_library_folders(&steam_root, &[&steam_root]);
    steam_library(&steam_root, &["Endless Sky.app/Contents/MacOS/Endless Sky"]);
    let installs = detect(&fake.env);
    assert_eq!(kinds(&installs), [InstallKind::Steam]);
    assert_eq!(
        installs[0].config_dir,
        fake.home("Library/Application Support/endless-sky")
    );
    assert!(installs[0].executable.is_some());
}

#[test]
fn custom_install_launch_passes_config_dir() {
    let install = GameInstall::custom(
        PathBuf::from("/data/es-test-profile"),
        Some(PathBuf::from("/opt/es/endless-sky")),
    );
    assert_eq!(install.kind, InstallKind::Custom);
    assert_eq!(
        install.plugins_dir(),
        Path::new("/data/es-test-profile/plugins")
    );
    let (program, args) = program_and_args(&launch_command(&install, Os::Linux).unwrap());
    assert_eq!(program, "/opt/es/endless-sky");
    assert_eq!(args, ["-c", "/data/es-test-profile"]);

    let no_exe = GameInstall::custom(PathBuf::from("/data/x"), None);
    assert!(launch_command(&no_exe, Os::Linux).is_none());
}

#[test]
fn standalone_from_user_chosen_executable() {
    let fake = Fake::new(Os::Linux);
    let appimage = fake.home("Applications/Endless_Sky-x86_64.AppImage");
    let install = GameInstall::standalone(&fake.env, appimage.clone());
    assert_eq!(install.config_dir, fake.home(".local/share/endless-sky"));
    let command = launch_command(&install, Os::Linux).unwrap();
    assert_eq!(command.get_program(), appimage.as_os_str());
    assert_eq!(command.get_args().count(), 0);
}

#[test]
fn steam_launch_command_per_os() {
    let install = GameInstall {
        kind: InstallKind::Steam,
        config_dir: PathBuf::from("/c"),
        executable: None,
        launch: Launch::Steam,
    };
    let cases: [(Os, &str, &[&str]); 3] = [
        (Os::Linux, "steam", &[STEAM_URL]),
        (Os::Windows, "cmd", &["/C", "start", "", STEAM_URL]),
        (Os::MacOs, "open", &[STEAM_URL]),
    ];
    for (os, program, args) in cases {
        let (p, a) = program_and_args(&launch_command(&install, os).unwrap());
        assert_eq!(p, program, "{os:?}");
        assert_eq!(a, args, "{os:?}");
    }
    assert_eq!(STEAM_URL, "steam://rungameid/404410");
}

#[test]
fn parses_version_output() {
    assert_eq!(
        parse_version_output(VERSION_OUTPUT).as_deref(),
        Some("0.11.3.0")
    );
    assert_eq!(
        parse_version_output("\nEndless Sky ver. 0.11.4.0-alpha\n").as_deref(),
        Some("0.11.4.0-alpha")
    );
    assert_eq!(
        parse_version_output("\r\nEndless Sky ver. 0.10.16\r\n").as_deref(),
        Some("0.10.16")
    );
    assert_eq!(parse_version_output("Usage: endless-sky [options]"), None);
    assert_eq!(parse_version_output(""), None);
}

#[cfg(unix)]
#[test]
fn query_version_runs_the_executable() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = TempDir::new().unwrap();
    let exe = tmp.path().join("endless-sky");
    let script = format!(
        "#!/bin/sh\n[ \"$1\" = --version ] || exit 1\nprintf '{}' >&2\n",
        VERSION_OUTPUT.replace('\n', "\\n")
    );
    fs::write(&exe, script).unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(query_version(&exe).as_deref(), Some("0.11.3.0"));
}

#[test]
fn query_version_of_missing_executable_is_none() {
    assert_eq!(query_version(Path::new("/nonexistent/endless-sky")), None);
}

#[test]
fn parses_real_library_folders() {
    let text = fs::read_to_string(Path::new(FIXTURES).join("libraryfolders.vdf")).unwrap();
    assert_eq!(
        parse_library_folders(&text),
        [
            PathBuf::from(r"C:\Program Files (x86)\Steam"),
            PathBuf::from(r"D:\SteamLibrary")
        ]
    );
}

#[test]
fn parses_legacy_library_folders() {
    let text = fs::read_to_string(Path::new(FIXTURES).join("libraryfolders-legacy.vdf")).unwrap();
    assert_eq!(
        parse_library_folders(&text),
        [
            PathBuf::from(r"D:\SteamLibrary"),
            PathBuf::from(r"E:\Games\Steam")
        ]
    );
}

#[test]
fn malformed_library_folders_does_not_panic() {
    for text in [
        "",
        "\"libraryfolders\"",
        "\"libraryfolders\" { \"0\" {",
        "}}}{",
    ] {
        let _ = parse_library_folders(text);
    }
    assert!(parse_library_folders("// comment\n\"other\" { }").is_empty());
}

#[test]
fn parses_reg_query_output() {
    let output = "\r\nHKEY_CURRENT_USER\\Software\\Valve\\Steam\r\n    SteamPath    REG_SZ    c:/program files (x86)/steam\r\n\r\n";
    assert_eq!(
        parse_reg_query(output),
        Some(PathBuf::from("c:/program files (x86)/steam"))
    );
    assert_eq!(parse_reg_query("ERROR: not found"), None);
}

#[test]
fn real_environment_detection_does_not_panic() {
    if let Some(env) = DetectEnv::from_system() {
        for install in detect(&env) {
            assert!(install.config_dir.ends_with(OsStr::new("endless-sky")));
        }
    }
}

const REAL_INFO_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
	<key>CFBundleName</key>
	<string>Endless Sky</string>
	<key>CFBundleShortVersionString</key>
	<string>0.11.2</string>
	<key>CFBundleVersion</key>
	<string>1</string>
</dict>
</plist>"#;

#[test]
fn parses_the_bundle_version_from_an_info_plist() {
    assert_eq!(
        parse_bundle_version(REAL_INFO_PLIST).as_deref(),
        Some("0.11.2")
    );
    assert_eq!(parse_bundle_version("<dict></dict>"), None);
    assert_eq!(parse_bundle_version("bplist00 binary"), None);
}

#[test]
fn reads_the_version_from_a_mac_bundle_without_running_it() {
    let tmp = TempDir::new().unwrap();
    let contents = tmp.path().join("Endless Sky.app").join("Contents");
    fs::create_dir_all(contents.join("MacOS")).unwrap();
    fs::write(contents.join("Info.plist"), REAL_INFO_PLIST).unwrap();
    // The executable is deliberately absent: the plist is enough.
    let exe = contents.join("MacOS").join("Endless Sky");
    assert_eq!(read_bundle_version(&exe).as_deref(), Some("0.11.2"));
}

#[test]
fn bundle_version_ignores_other_layouts() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("Info.plist"), REAL_INFO_PLIST).unwrap();
    assert_eq!(read_bundle_version(&tmp.path().join("endless-sky")), None);
}
