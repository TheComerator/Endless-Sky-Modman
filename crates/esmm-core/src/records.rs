//! The manager's own record of each plugin it installed (decision A).
//!
//! Stored in the manager's app-data dir, never inside a plugin folder.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallRecord {
    pub catalog_name: String,
    pub folder: String,
    /// `plugin.txt` name, or the folder name when there is none.
    pub identity: String,
    pub version: String,
    pub source_url: String,
    /// Lowercase hex SHA-256 of the downloaded zip.
    pub sha256: String,
}

/// Keyed by installed folder name.
pub type InstallRecords = BTreeMap<String, InstallRecord>;

#[derive(Debug, thiserror::Error)]
pub enum RecordsError {
    #[error("failed to read or write install records: {0}")]
    Io(#[from] io::Error),
    #[error("install records file is corrupt: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn load(path: &Path) -> Result<InstallRecords, RecordsError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(serde_json::from_str(&text)?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(InstallRecords::new()),
        Err(e) => Err(e.into()),
    }
}

pub fn save(path: &Path, records: &InstallRecords) -> Result<(), RecordsError> {
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer_pretty(&mut tmp, records)?;
    tmp.write_all(b"\n")?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&dir.path().join("records.json")).unwrap().is_empty());
    }

    #[test]
    fn save_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app-data/records.json");
        let record = InstallRecord {
            catalog_name: "Jimmys-Ship-Emporium".into(),
            folder: "Jimmys-Ship-Emporium".into(),
            identity: "Jimmy's Ship Emporium".into(),
            version: "v0.1.1".into(),
            source_url: "https://example.com/a.zip".into(),
            sha256: "ab".repeat(32),
        };
        let mut records = InstallRecords::new();
        records.insert(record.folder.clone(), record);
        save(&path, &records).unwrap();
        assert_eq!(load(&path).unwrap(), records);

        records.clear();
        save(&path, &records).unwrap();
        assert!(load(&path).unwrap().is_empty());
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }
}
