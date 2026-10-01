//! Atomic file writes and JSON load/save for the manager's own files.

use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Write via a temp file in the same directory, then rename over `path`.
pub(crate) fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(dir)?;
    let mut prefix = path.file_name().unwrap_or_default().to_os_string();
    prefix.push(".esmm-");
    let mut tmp = tempfile::Builder::new().prefix(&prefix).tempfile_in(dir)?;
    tmp.write_all(contents)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// A missing file loads as `T::default()`.
pub(crate) fn load_json<T, E>(path: &Path) -> Result<T, E>
where
    T: DeserializeOwned + Default,
    E: From<io::Error> + From<serde_json::Error>,
{
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(serde_json::from_str(&text)?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e.into()),
    }
}

pub(crate) fn save_json<T, E>(path: &Path, value: &T) -> Result<(), E>
where
    T: Serialize,
    E: From<io::Error> + From<serde_json::Error>,
{
    let mut json = serde_json::to_vec_pretty(value)?;
    json.push(b'\n');
    Ok(write_atomic(path, &json)?)
}
