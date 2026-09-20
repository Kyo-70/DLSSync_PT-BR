use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

const MANIFEST: &str = "snapshot.sha256.json";

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    path: PathBuf,
    bytes: u64,
    sha256: String,
}

fn collect(root: &Path, current: &Path, entries: &mut Vec<Entry>) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(current).map_err(|error| error.to_string())?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Driver snapshot contains a reparse point".into());
        }
    }
    if metadata.file_type().is_symlink() {
        return Err("Driver snapshot contains a symbolic link".into());
    }
    if metadata.is_dir() {
        for entry in std::fs::read_dir(current).map_err(|error| error.to_string())? {
            collect(
                root,
                &entry.map_err(|error| error.to_string())?.path(),
                entries,
            )?;
        }
    } else if metadata.is_file() && current != root.join(MANIFEST) {
        if entries.len() >= 10_000 {
            return Err("Driver snapshot contains too many files".into());
        }
        let mut file = std::fs::File::open(current).map_err(|error| error.to_string())?;
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        let mut bytes = 0;
        loop {
            let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            bytes += count as u64;
            hash.update(&buffer[..count]);
        }
        entries.push(Entry {
            path: current
                .strip_prefix(root)
                .map_err(|error| error.to_string())?
                .to_path_buf(),
            bytes,
            sha256: format!("{:x}", hash.finalize()),
        });
    }
    Ok(())
}

fn inventory(root: &Path) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    collect(root, root, &mut entries)?;
    if !entries.iter().any(|entry| {
        entry
            .path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("inf"))
    }) {
        return Err("Driver snapshot contains no INF package".into());
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

/// Seal exact exported bytes. This detects changes; it does not replace Windows publisher checks.
pub fn seal_driver_snapshot(root: &Path) -> Result<u64, String> {
    let entries = inventory(root)?;
    let bytes = entries.iter().map(|entry| entry.bytes).sum();
    let data = serde_json::to_vec_pretty(&entries).map_err(|error| error.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(MANIFEST))
        .map_err(|error| error.to_string())?;
    std::io::Write::write_all(&mut file, &data).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    Ok(bytes)
}

pub fn verify_driver_snapshot(root: &Path) -> Result<u64, String> {
    let current = inventory(root)?;
    let bytes = std::fs::read(root.join(MANIFEST))
        .map_err(|error| format!("Driver snapshot has no readable file manifest: {error}"))?;
    let expected: Vec<Entry> = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if current != expected {
        return Err("Driver snapshot files do not match their saved SHA-256 values".into());
    }
    Ok(current.iter().map(|entry| entry.bytes).sum())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_missing_modified_and_additional_files() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("driver.inf"), b"original").unwrap();
        assert!(verify_driver_snapshot(root.path()).is_err());
        seal_driver_snapshot(root.path()).unwrap();
        assert_eq!(verify_driver_snapshot(root.path()).unwrap(), 8);
        std::fs::write(root.path().join("driver.inf"), b"modified").unwrap();
        assert!(verify_driver_snapshot(root.path()).is_err());
        std::fs::write(root.path().join("driver.inf"), b"original").unwrap();
        std::fs::write(root.path().join("extra.dll"), b"extra").unwrap();
        assert!(verify_driver_snapshot(root.path()).is_err());
    }
}
