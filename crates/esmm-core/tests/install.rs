//! Installer tests against zips built on the fly in temp dirs.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use esmm_core::install::{
    self, ExtractLimits, InstallError, extract_zip, find_plugin_root, sanitize_folder_name,
};
use zip::write::SimpleFileOptions;

enum Entry<'a> {
    File(&'a str, &'a str),
    Dir(&'a str),
    Symlink(&'a str, &'a str),
}

fn make_zip(path: &Path, entries: &[Entry]) {
    let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
    let opts = SimpleFileOptions::default();
    for entry in entries {
        match entry {
            Entry::File(name, body) => {
                zip.start_file(*name, opts).unwrap();
                zip.write_all(body.as_bytes()).unwrap();
            }
            Entry::Dir(name) => zip.add_directory(*name, opts).unwrap(),
            Entry::Symlink(name, target) => zip.add_symlink(*name, *target, opts).unwrap(),
        }
    }
    zip.finish().unwrap();
}

/// A fake game config dir with a zip built from `entries`.
struct Setup {
    _dir: tempfile::TempDir,
    config: PathBuf,
    zip: PathBuf,
}

impl Setup {
    fn new(entries: &[Entry]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("endless-sky");
        fs::create_dir_all(install::plugins_dir(&config)).unwrap();
        let zip = dir.path().join("download.zip");
        make_zip(&zip, entries);
        Self {
            _dir: dir,
            config,
            zip,
        }
    }

    fn plugins(&self) -> PathBuf {
        install::plugins_dir(&self.config)
    }

    fn tmp(&self) -> PathBuf {
        install::tmp_dir(&self.config)
    }

    fn install(&self, catalog_name: &str) -> install::Installed {
        let staging = install::new_staging_dir(&self.tmp()).unwrap();
        extract_zip(&self.zip, staging.path(), ExtractLimits::default()).unwrap();
        let root = find_plugin_root(staging.path()).unwrap();
        install::install(
            &root,
            &self.plugins(),
            &self.tmp(),
            &sanitize_folder_name(catalog_name),
        )
        .unwrap()
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

const WRAPPED: &[Entry] = &[
    Entry::Dir("Foo-1.2.3/"),
    Entry::File("Foo-1.2.3/data/x.txt", "ship stuff"),
    Entry::File(
        "Foo-1.2.3/plugin.txt",
        "name \"Foo Plugin\"\nversion 1.2.3\n",
    ),
];

#[test]
fn wrapper_folder_is_stripped_and_version_never_in_name() {
    let s = Setup::new(WRAPPED);
    let installed = s.install("Foo: The Plugin");
    assert_eq!(installed.folder, "Foo_ The Plugin");
    assert_eq!(installed.path, s.plugins().join("Foo_ The Plugin"));
    assert_eq!(installed.identity(), "Foo Plugin");
    assert_eq!(
        fs::read_to_string(installed.path.join("data/x.txt")).unwrap(),
        "ship stuff"
    );
    assert_eq!(names(&s.plugins()), ["Foo_ The Plugin"]);
    assert!(names(&s.tmp()).is_empty());
}

#[test]
fn plugin_at_zip_root_without_plugin_txt() {
    let s = Setup::new(&[
        Entry::File("images/ship.png", "png"),
        Entry::File("about.txt", "hi"),
    ]);
    let installed = s.install("Root Plugin");
    assert!(installed.meta.is_none());
    assert_eq!(installed.identity(), "Root Plugin");
    assert!(installed.path.join("images/ship.png").is_file());
}

#[test]
fn zip_slip_and_symlink_entries_stay_inside_staging() {
    let dir = tempfile::tempdir().unwrap();
    let staging = dir.path().join("stage");
    let absolute = dir.path().join("abs-evil.txt");
    let absolute = absolute.to_str().unwrap();
    // The zip writer strips a leading '/', so write a placeholder and patch the name bytes.
    let placeholder = format!("X{}", &absolute[1..]);
    let zip = dir.path().join("evil.zip");
    make_zip(
        &zip,
        &[
            Entry::File("../evil.txt", "x"),
            Entry::File("ok/../../evil2.txt", "x"),
            Entry::File(&placeholder, "x"),
            Entry::Symlink("data/link", "/etc/passwd"),
            Entry::File("data/good.txt", "fine"),
        ],
    );
    let bytes = fs::read(&zip).unwrap();
    let mut patched = Vec::with_capacity(bytes.len());
    let mut rest = &bytes[..];
    while let Some(i) = rest
        .windows(placeholder.len())
        .position(|w| w == placeholder.as_bytes())
    {
        patched.extend_from_slice(&rest[..i]);
        patched.extend_from_slice(absolute.as_bytes());
        rest = &rest[i + placeholder.len()..];
    }
    patched.extend_from_slice(rest);
    fs::write(&zip, patched).unwrap();

    let report = extract_zip(&zip, &staging, ExtractLimits::default()).unwrap();
    assert_eq!(report.skipped.len(), 3);
    // zip's enclosed_name strips the root, so the absolute entry lands inside staging.
    assert_eq!(report.files, 2);
    // Strip the root/prefix (e.g. `/` on Unix, `C:\` on Windows) the same way enclosed_name
    // does, instead of assuming a single-character root.
    let expected_relative: PathBuf = Path::new(absolute)
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s),
            _ => None,
        })
        .collect();
    assert!(staging.join(&expected_relative).is_file());
    assert!(!dir.path().join("evil.txt").exists());
    assert!(!dir.path().join("evil2.txt").exists());
    assert!(!Path::new(absolute).exists());
    assert!(fs::symlink_metadata(staging.join("data/link")).is_err());
    assert_eq!(names(dir.path()), ["evil.zip", "stage"]);
    assert_eq!(names(&staging.join("data")), ["good.txt"]);
}

#[test]
fn no_plugin_dirs_is_not_a_plugin() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("Foo-1.0/docs")).unwrap();
    fs::write(dir.path().join("Foo-1.0/README.md"), "x").unwrap();
    assert!(matches!(
        find_plugin_root(dir.path()),
        Err(InstallError::NotAPlugin)
    ));
}

#[test]
fn plugin_deeper_than_two_levels_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("a/b/c/data")).unwrap();
    assert!(matches!(
        find_plugin_root(dir.path()),
        Err(InstallError::NotAPlugin)
    ));
}

#[test]
fn two_level_nesting_is_found() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("a/b/sounds")).unwrap();
    assert_eq!(
        find_plugin_root(dir.path()).unwrap(),
        dir.path().join("a/b")
    );
}

#[test]
fn sibling_plugins_are_ambiguous() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("Pack/One/data")).unwrap();
    fs::create_dir_all(dir.path().join("Pack/Two/images")).unwrap();
    match find_plugin_root(dir.path()) {
        Err(InstallError::Ambiguous(found)) => assert_eq!(
            found,
            [dir.path().join("Pack/One"), dir.path().join("Pack/Two")]
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn shallowest_plugin_wins() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("Foo/data")).unwrap();
    fs::create_dir_all(dir.path().join("Foo/extras/Bar/data")).unwrap();
    assert_eq!(
        find_plugin_root(dir.path()).unwrap(),
        dir.path().join("Foo")
    );
}

#[test]
fn size_limit_triggers() {
    let s = Setup::new(&[
        Entry::File("data/a.txt", &"a".repeat(600)),
        Entry::File("data/b.txt", &"b".repeat(600)),
    ]);
    let staging = tempfile::tempdir().unwrap();
    let limits = ExtractLimits {
        max_total_bytes: 1000,
        ..ExtractLimits::default()
    };
    assert!(matches!(
        extract_zip(&s.zip, staging.path(), limits),
        Err(InstallError::TooLarge(1000))
    ));
    let exact = ExtractLimits {
        max_total_bytes: 1200,
        ..ExtractLimits::default()
    };
    assert_eq!(
        extract_zip(&s.zip, staging.path(), exact).unwrap().bytes,
        1200
    );
}

#[test]
fn entry_limit_triggers() {
    let s = Setup::new(&[
        Entry::File("data/a.txt", "a"),
        Entry::File("data/b.txt", "b"),
        Entry::File("data/c.txt", "c"),
    ]);
    let staging = tempfile::tempdir().unwrap();
    let limits = ExtractLimits {
        max_entries: 2,
        ..ExtractLimits::default()
    };
    assert!(matches!(
        extract_zip(&s.zip, staging.path(), limits),
        Err(InstallError::TooManyEntries(2))
    ));
    assert!(names(staging.path()).is_empty());
}

#[test]
fn reinstall_replaces_and_leaves_nothing_behind() {
    let s = Setup::new(WRAPPED);
    let first = s.install("Foo");
    fs::write(first.path.join("data/stale.txt"), "old version file").unwrap();

    let v2 = Setup::new(&[Entry::File("Foo-2.0.0/data/y.txt", "new")]);
    let staging = install::new_staging_dir(&s.tmp()).unwrap();
    extract_zip(&v2.zip, staging.path(), ExtractLimits::default()).unwrap();
    let root = find_plugin_root(staging.path()).unwrap();
    let second = install::install(&root, &s.plugins(), &s.tmp(), "Foo").unwrap();
    drop(staging);

    assert_eq!(second.path, first.path);
    assert!(second.meta.is_none());
    assert_eq!(names(&second.path.join("data")), ["y.txt"]);
    assert_eq!(second.leftover, None);
    assert_eq!(names(&s.plugins()), ["Foo"]);
    assert!(names(&s.tmp()).is_empty());
}

#[cfg(unix)]
#[test]
fn failed_swap_restores_the_old_version() {
    use std::os::unix::fs::PermissionsExt;

    let s = Setup::new(WRAPPED);
    let first = s.install("Foo");
    // A read-only parent makes the staged folder impossible to move, after the old copy is out.
    let locked = s.tmp().join("locked");
    let staged = locked.join("Foo-2.0");
    fs::create_dir_all(staged.join("data")).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
    if fs::write(locked.join("probe"), "").is_ok() {
        eprintln!("skipping: permissions not enforced (running as root?)");
        return;
    }

    let result = install::install(&staged, &s.plugins(), &s.tmp(), "Foo");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        matches!(result, Err(InstallError::Io(ref e)) if e.kind() == std::io::ErrorKind::PermissionDenied),
        "{result:?}"
    );
    assert_eq!(
        fs::read_to_string(first.path.join("data/x.txt")).unwrap(),
        "ship stuff"
    );
    assert_eq!(names(&s.plugins()), ["Foo"]);
    assert_eq!(names(&s.tmp()), ["locked"], "old-copy holder cleaned up");
}

#[test]
fn install_rejects_unsafe_folder_names_and_non_plugins() {
    let s = Setup::new(WRAPPED);
    let staged = s.tmp().join("x");
    fs::create_dir_all(staged.join("data")).unwrap();
    for bad in ["..", "a/b", "CON", "", "x."] {
        assert!(matches!(
            install::install(&staged, &s.plugins(), &s.tmp(), bad),
            Err(InstallError::InvalidFolderName(_))
        ));
    }
    let empty = s.tmp().join("empty");
    fs::create_dir_all(&empty).unwrap();
    assert!(matches!(
        install::install(&empty, &s.plugins(), &s.tmp(), "Foo"),
        Err(InstallError::NotAPlugin)
    ));
    assert!(names(&s.plugins()).is_empty());
}

#[test]
fn uninstall_removes_folder_and_temp_copy() {
    let s = Setup::new(WRAPPED);
    let installed = s.install("Foo");
    install::uninstall(&s.plugins(), &s.tmp(), &installed.folder).unwrap();
    assert!(names(&s.plugins()).is_empty());
    assert!(names(&s.tmp()).is_empty());
    assert!(matches!(
        install::uninstall(&s.plugins(), &s.tmp(), "Foo"),
        Err(InstallError::NotInstalled(_))
    ));
}

#[test]
fn sanitize_folder_name_cases() {
    for (input, expected) in [
        ("A: B?", "A_ B_"),
        ("CON", "_CON"),
        ("con.txt", "_con.txt"),
        ("lpt9", "_lpt9"),
        ("COM0", "COM0"),
        ("Console", "Console"),
        ("name. ", "name"),
        ("Jimmy's Ship Emporium", "Jimmy's Ship Emporium"),
        ("", "_"),
        ("...", "_"),
        ("a<b>c\"d/e\\f|g*h\ti", "a_b_c_d_e_f_g_h_i"),
        ("Jimmys-Ship-Emporium", "Jimmys-Ship-Emporium"),
        ("COM\u{b9}", "_COM\u{b9}"),
        ("lpt\u{b3}.txt", "_lpt\u{b3}.txt"),
        ("COM\u{b9}x", "COM\u{b9}x"),
        ("COM\u{2074}", "COM\u{2074}"),
    ] {
        assert_eq!(sanitize_folder_name(input), expected, "{input:?}");
    }
}

#[test]
fn sanitize_folder_name_caps_length() {
    assert_eq!(sanitize_folder_name(&"a".repeat(300)), "a".repeat(255));
    // 'é' is 2 bytes, so 255 bytes would split one; the cut falls back to 254.
    assert_eq!(sanitize_folder_name(&"é".repeat(200)), "é".repeat(127));
    let dots_at_cut = format!("{}. .{}", "a".repeat(252), "b".repeat(50));
    assert_eq!(sanitize_folder_name(&dots_at_cut), "a".repeat(252));
    let reserved = format!("CON.{}", "x".repeat(300));
    let got = sanitize_folder_name(&reserved);
    assert_eq!(got.len(), 255);
    assert!(got.starts_with("_CON.x"));
    for name in [&"a".repeat(300), &"é".repeat(200), &dots_at_cut, &reserved] {
        let once = sanitize_folder_name(name);
        assert_eq!(
            sanitize_folder_name(&once),
            once,
            "stable when re-sanitized"
        );
    }
}
