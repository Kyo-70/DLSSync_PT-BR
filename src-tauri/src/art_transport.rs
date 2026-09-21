use crate::constants::{
    ART_PROTOCOL_CACHE_DIR, ART_PROTOCOL_CACHE_MAX_BYTES, ART_PROTOCOL_CACHE_PARENT,
    ART_PROTOCOL_CACHE_TTL_SECS, ART_PROTOCOL_MAX_ASSET_BYTES,
};
use launcher_scan::{ArtLocatorKind, GameArt, GameArtAsset};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Manager, Runtime};

#[derive(Debug, thiserror::Error)]
pub enum ArtTransportError {
    #[error("art asset is not a regular file")]
    NotAFile,
    #[error("art asset exceeds the per-file cache limit")]
    AssetTooLarge,
    #[error("art asset bytes are not a verified PNG or JPEG")]
    InvalidImage,
    #[error("art transport cache has reached its size limit")]
    CacheFull,
    #[error("art transport I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("asset protocol scope: {0}")]
    Scope(#[from] tauri::Error),
}

#[derive(Debug)]
struct CacheFile {
    path: PathBuf,
    len: u64,
    modified: SystemTime,
}

pub fn transport_root(cache_dir: &Path) -> PathBuf {
    cache_dir
        .join(ART_PROTOCOL_CACHE_PARENT)
        .join(ART_PROTOCOL_CACHE_DIR)
}

pub fn initialize<R: Runtime>(
    app: &AppHandle<R>,
    cache_dir: &Path,
) -> Result<PathBuf, ArtTransportError> {
    let root = transport_root(cache_dir);
    fs::create_dir_all(&root)?;
    cleanup_expired(&root, &HashSet::new())?;
    cleanup_to_limit(&root, &HashSet::new(), 0)?;
    app.asset_protocol_scope().allow_directory(&root, false)?;
    Ok(root)
}

pub fn prepare_art(cache_dir: &Path, art: &mut GameArt) -> Result<(), ArtTransportError> {
    let mut session = ArtTransportSession::new(cache_dir)?;
    session.prepare(art)?;
    session.finish()
}

pub struct ArtTransportSession {
    root: PathBuf,
    protected: HashSet<PathBuf>,
}

impl ArtTransportSession {
    pub fn new(cache_dir: &Path) -> Result<Self, ArtTransportError> {
        let root = transport_root(cache_dir);
        fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            protected: HashSet::new(),
        })
    }

    pub fn prepare(&mut self, art: &mut GameArt) -> Result<(), ArtTransportError> {
        for asset in [art.landscape.as_mut(), art.portrait.as_mut()]
            .into_iter()
            .flatten()
        {
            prepare_asset(&self.root, asset, &mut self.protected)?;
        }
        Ok(())
    }

    pub fn finish(self) -> Result<(), ArtTransportError> {
        cleanup_expired(&self.root, &self.protected)?;
        cleanup_to_limit(&self.root, &self.protected, 0)
    }
}

pub fn persist_verified_bytes(
    cache_dir: &Path,
    bytes: &[u8],
) -> Result<PathBuf, ArtTransportError> {
    let root = transport_root(cache_dir);
    fs::create_dir_all(&root)?;
    persist_bytes(&root, bytes, &mut HashSet::new())
}

fn prepare_asset(
    root: &Path,
    asset: &mut GameArtAsset,
    protected: &mut HashSet<PathBuf>,
) -> Result<(), ArtTransportError> {
    if asset.locator_kind != ArtLocatorKind::LocalFile || !asset.verified {
        return Err(ArtTransportError::InvalidImage);
    }
    let source = Path::new(&asset.locator);
    let metadata = fs::symlink_metadata(source)?;
    if !metadata.file_type().is_file() {
        return Err(ArtTransportError::NotAFile);
    }
    if metadata.len() > ART_PROTOCOL_MAX_ASSET_BYTES {
        return Err(ArtTransportError::AssetTooLarge);
    }
    let bytes = fs::read(source)?;
    let (width, height) =
        launcher_scan::art::verified_image_bytes(&bytes).ok_or(ArtTransportError::InvalidImage)?;
    let destination = persist_bytes(root, &bytes, protected)?;
    asset.locator = destination.to_string_lossy().into_owned();
    asset.width = width;
    asset.height = height;
    Ok(())
}

fn persist_bytes(
    root: &Path,
    bytes: &[u8],
    protected: &mut HashSet<PathBuf>,
) -> Result<PathBuf, ArtTransportError> {
    if bytes.len() as u64 > ART_PROTOCOL_MAX_ASSET_BYTES {
        return Err(ArtTransportError::AssetTooLarge);
    }
    let extension = verified_extension(bytes).ok_or(ArtTransportError::InvalidImage)?;
    let digest = Sha256::digest(bytes);
    let stem = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let destination = root.join(format!("{stem}.{extension}"));
    protected.insert(destination.clone());

    if let Ok(metadata) = fs::symlink_metadata(&destination) {
        if metadata.file_type().is_file()
            && metadata.len() == bytes.len() as u64
            && fs::read(&destination).is_ok_and(|existing| existing == bytes)
        {
            return Ok(destination);
        }
        fs::remove_file(&destination)?;
    }

    cleanup_expired(root, protected)?;
    cleanup_to_limit(root, protected, bytes.len() as u64)?;

    let temporary = root.join(format!(".{stem}.{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<(), ArtTransportError> {
        let mut file = File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, &destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(destination)
}

fn verified_extension(bytes: &[u8]) -> Option<&'static str> {
    launcher_scan::art::verified_image_bytes(bytes)?;
    if bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]) {
        Some("png")
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        Some("jpg")
    } else {
        None
    }
}

fn cleanup_expired(root: &Path, protected: &HashSet<PathBuf>) -> Result<(), ArtTransportError> {
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(ART_PROTOCOL_CACHE_TTL_SECS))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    for entry in cache_files(root)? {
        if !protected.contains(&entry.path) && entry.modified < cutoff {
            fs::remove_file(entry.path)?;
        }
    }
    Ok(())
}

fn cleanup_to_limit(
    root: &Path,
    protected: &HashSet<PathBuf>,
    incoming_bytes: u64,
) -> Result<(), ArtTransportError> {
    let mut files = cache_files(root)?;
    let mut total = files.iter().map(|file| file.len).sum::<u64>();
    files.sort_by_key(|file| file.modified);
    for file in files {
        if total.saturating_add(incoming_bytes) <= ART_PROTOCOL_CACHE_MAX_BYTES {
            return Ok(());
        }
        if protected.contains(&file.path) {
            continue;
        }
        fs::remove_file(&file.path)?;
        total = total.saturating_sub(file.len);
    }
    if total.saturating_add(incoming_bytes) > ART_PROTOCOL_CACHE_MAX_BYTES {
        Err(ArtTransportError::CacheFull)
    } else {
        Ok(())
    }
}

fn cache_files(root: &Path) -> Result<Vec<CacheFile>, ArtTransportError> {
    let mut files = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_file() {
            files.push(CacheFile {
                path: entry.path(),
                len: metadata.len(),
                modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            });
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use launcher_scan::{GameArt, GameArtAsset, GameArtSource};
    use tempfile::tempdir;

    fn png(width: u32, height: u32, len: usize) -> Vec<u8> {
        let mut bytes = vec![0_u8; len.max(24)];
        bytes[..8].copy_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        bytes
    }

    #[test]
    fn materializes_verified_art_only_inside_transport_root() {
        let temp = tempdir().unwrap();
        let launcher_cache = temp.path().join("launcher-cache");
        fs::create_dir_all(&launcher_cache).unwrap();
        let source = launcher_cache.join("cover.png");
        fs::write(&source, png(1920, 620, 128)).unwrap();
        let cache = temp.path().join("app").join("Cache");
        let mut art = GameArt::resolved(
            Some(GameArtAsset::local(
                &source,
                GameArtSource::SteamLibraryCache,
                "library_hero",
                1,
                1,
            )),
            None,
        );

        prepare_art(&cache, &mut art).unwrap();

        let locator = PathBuf::from(&art.landscape.unwrap().locator);
        assert!(locator.starts_with(transport_root(&cache)));
        assert!(!locator.starts_with(&launcher_cache));
    }

    #[test]
    fn rejects_an_asset_over_the_per_file_limit() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("too-large.png");
        fs::write(
            &source,
            png(1920, 620, ART_PROTOCOL_MAX_ASSET_BYTES as usize + 1),
        )
        .unwrap();
        let mut art = GameArt::resolved(
            Some(GameArtAsset::local(
                &source,
                GameArtSource::SteamLibraryCache,
                "library_hero",
                1920,
                620,
            )),
            None,
        );

        assert!(matches!(
            prepare_art(&temp.path().join("Cache"), &mut art),
            Err(ArtTransportError::AssetTooLarge)
        ));
    }

    #[test]
    fn cleanup_never_removes_a_sibling_outside_the_transport_root() {
        let temp = tempdir().unwrap();
        let cache = temp.path().join("Cache");
        let sibling = cache.join("keep.json");
        fs::create_dir_all(&cache).unwrap();
        fs::write(&sibling, b"keep").unwrap();
        let root = transport_root(&cache);
        fs::create_dir_all(&root).unwrap();

        cleanup_to_limit(&root, &HashSet::new(), 0).unwrap();

        assert_eq!(fs::read(&sibling).unwrap(), b"keep");
    }
}
