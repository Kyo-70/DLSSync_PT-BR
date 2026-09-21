use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum GameArtState {
    Resolved,
    Pending,
    Unavailable,
    SourceFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum GameArtSource {
    SteamLibraryCache,
    SteamOfficialCdn,
    EpicManifest,
    EpicCatalog,
    GogRegistry,
    UbisoftRegistry,
    EaRegistry,
    XboxPackage,
    BattlenetProductDb,
    SteamGridDb,
    ManualFolder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ArtLocatorKind {
    LocalFile,
    HttpsUrl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ArtCacheStatus {
    NotChecked,
    Miss,
    Hit,
    Stored,
    TransientFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ArtResolveTrigger {
    Automatic,
    UserScan,
    ExplicitRetry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct GameArtAsset {
    pub locator: String,
    pub locator_kind: ArtLocatorKind,
    pub source: GameArtSource,
    pub variant: String,
    pub width: u32,
    pub height: u32,
    pub verified: bool,
}

impl GameArtAsset {
    pub fn local(
        path: &Path,
        source: GameArtSource,
        variant: impl Into<String>,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            locator: path.to_string_lossy().into_owned(),
            locator_kind: ArtLocatorKind::LocalFile,
            source,
            variant: variant.into(),
            width,
            height,
            verified: true,
        }
    }

    pub fn remote(
        url: impl Into<String>,
        source: GameArtSource,
        variant: impl Into<String>,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            locator: url.into(),
            locator_kind: ArtLocatorKind::HttpsUrl,
            source,
            variant: variant.into(),
            width,
            height,
            verified: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct GameArtCandidate {
    pub locator: String,
    pub source: GameArtSource,
    pub variant: String,
    pub width: u32,
    pub height: u32,
}

impl GameArtCandidate {
    pub fn https(
        url: impl Into<String>,
        source: GameArtSource,
        variant: impl Into<String>,
        width: u32,
        height: u32,
    ) -> Option<Self> {
        let url = url.into();
        url.starts_with("https://").then(|| Self {
            locator: url,
            source,
            variant: variant.into(),
            width,
            height,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct GameArt {
    pub state: GameArtState,
    pub landscape: Option<GameArtAsset>,
    pub portrait: Option<GameArtAsset>,
    #[serde(default)]
    pub candidates: Vec<GameArtCandidate>,
    pub error_code: Option<String>,
    pub retryable: bool,
    pub cache_status: ArtCacheStatus,
}

impl Default for GameArt {
    fn default() -> Self {
        Self::pending(Vec::new())
    }
}

impl GameArt {
    pub fn resolved(landscape: Option<GameArtAsset>, portrait: Option<GameArtAsset>) -> Self {
        debug_assert!(landscape.is_some() || portrait.is_some());
        Self {
            state: GameArtState::Resolved,
            landscape,
            portrait,
            candidates: Vec::new(),
            error_code: None,
            retryable: false,
            cache_status: ArtCacheStatus::NotChecked,
        }
    }

    pub fn pending(candidates: Vec<GameArtCandidate>) -> Self {
        Self {
            state: GameArtState::Pending,
            landscape: None,
            portrait: None,
            candidates,
            error_code: None,
            retryable: true,
            cache_status: ArtCacheStatus::NotChecked,
        }
    }

    pub fn unavailable(source: GameArtSource, reason: &str) -> Self {
        Self {
            state: GameArtState::Unavailable,
            landscape: None,
            portrait: None,
            candidates: Vec::new(),
            error_code: Some(format!("{}:{reason}", source_code(source))),
            retryable: false,
            cache_status: ArtCacheStatus::NotChecked,
        }
    }

    pub fn source_failed(source: GameArtSource, code: &str) -> Self {
        Self {
            state: GameArtState::SourceFailed,
            landscape: None,
            portrait: None,
            candidates: Vec::new(),
            error_code: Some(format!("{}:{code}", source_code(source))),
            retryable: true,
            cache_status: ArtCacheStatus::TransientFailure,
        }
    }

    pub fn preferred(&self) -> Option<&GameArtAsset> {
        self.landscape.as_ref().or(self.portrait.as_ref())
    }
}

fn source_code(source: GameArtSource) -> &'static str {
    match source {
        GameArtSource::SteamLibraryCache => "steam_library_cache",
        GameArtSource::SteamOfficialCdn => "steam_official_cdn",
        GameArtSource::EpicManifest => "epic_manifest",
        GameArtSource::EpicCatalog => "epic_catalog",
        GameArtSource::GogRegistry => "gog_registry",
        GameArtSource::UbisoftRegistry => "ubisoft_registry",
        GameArtSource::EaRegistry => "ea_registry",
        GameArtSource::XboxPackage => "xbox_package",
        GameArtSource::BattlenetProductDb => "battlenet_product_db",
        GameArtSource::SteamGridDb => "steamgriddb",
        GameArtSource::ManualFolder => "manual_folder",
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LocalVariant<'a> {
    pub file_name: &'a str,
    pub variant: &'a str,
    pub nominal_width: u32,
    pub nominal_height: u32,
}

pub fn best_local_asset(
    root: &Path,
    variants: &[LocalVariant<'_>],
    source: GameArtSource,
) -> Option<GameArtAsset> {
    if !root.is_dir() {
        return None;
    }
    let mut files = Vec::new();
    collect_files(root, 0, 4, &mut files);
    variants
        .iter()
        .flat_map(|variant| {
            files
                .iter()
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.eq_ignore_ascii_case(variant.file_name))
                })
                .filter_map(move |path| {
                    verified_dimensions(path).map(|(width, height)| {
                        GameArtAsset::local(path, source, variant.variant, width, height)
                    })
                })
        })
        .max_by(asset_resolution_order)
}

fn collect_files(root: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
    if depth > max_depth {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            out.push(path);
        } else if path.is_dir() {
            collect_files(&path, depth + 1, max_depth, out);
        }
    }
}

fn asset_resolution_order(left: &GameArtAsset, right: &GameArtAsset) -> Ordering {
    u64::from(left.width)
        .saturating_mul(u64::from(left.height))
        .cmp(&u64::from(right.width).saturating_mul(u64::from(right.height)))
}

pub fn verified_dimensions(path: &Path) -> Option<(u32, u32)> {
    let bytes = std::fs::read(path).ok()?;
    verified_image_bytes(&bytes)
}

pub fn verified_image_bytes(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() >= 24 && bytes[..8] == [137, 80, 78, 71, 13, 10, 26, 10] {
        let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        return (width > 0 && height > 0).then_some((width, height));
    }
    if bytes.len() >= 2 && bytes[..2] == [0xff, 0xd8] {
        let mut cursor = Cursor::new(bytes);
        cursor.seek(SeekFrom::Start(2)).ok()?;
        return jpeg_dimensions(&mut cursor);
    }
    None
}

fn jpeg_dimensions<R: Read + Seek>(reader: &mut R) -> Option<(u32, u32)> {
    loop {
        let mut marker = [0_u8; 2];
        reader.read_exact(&mut marker).ok()?;
        while marker[0] != 0xff {
            marker[0] = marker[1];
            reader.read_exact(&mut marker[1..2]).ok()?;
        }
        while marker[1] == 0xff {
            reader.read_exact(&mut marker[1..2]).ok()?;
        }
        if marker[1] == 0xd9 || marker[1] == 0xda {
            return None;
        }
        let mut length = [0_u8; 2];
        reader.read_exact(&mut length).ok()?;
        let segment_len = u16::from_be_bytes(length);
        if segment_len < 2 {
            return None;
        }
        if matches!(
            marker[1],
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) {
            let mut dimensions = [0_u8; 5];
            reader.read_exact(&mut dimensions).ok()?;
            let height = u16::from_be_bytes([dimensions[1], dimensions[2]]) as u32;
            let width = u16::from_be_bytes([dimensions[3], dimensions[4]]) as u32;
            return (width > 0 && height > 0).then_some((width, height));
        }
        reader
            .seek(SeekFrom::Current(i64::from(segment_len) - 2))
            .ok()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_or_empty_file_is_not_verified_art() {
        let path = std::env::temp_dir().join("dlssync-invalid-art.bin");
        std::fs::write(&path, b"not an image").unwrap();
        assert_eq!(verified_dimensions(&path), None);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn local_variants_choose_highest_verified_pixel_count() {
        let root = std::env::temp_dir().join("dlssync-local-art-resolution-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for (name, width, height) in [("small.png", 460_u32, 215_u32), ("large.png", 1920, 620)] {
            let mut png = vec![0_u8; 24];
            png[..8].copy_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
            png[16..20].copy_from_slice(&width.to_be_bytes());
            png[20..24].copy_from_slice(&height.to_be_bytes());
            std::fs::write(root.join(name), png).unwrap();
        }
        let variants = [
            LocalVariant {
                file_name: "small.png",
                variant: "small",
                nominal_width: 460,
                nominal_height: 215,
            },
            LocalVariant {
                file_name: "large.png",
                variant: "large",
                nominal_width: 1920,
                nominal_height: 620,
            },
        ];
        let asset = best_local_asset(&root, &variants, GameArtSource::XboxPackage).unwrap();
        assert_eq!(asset.variant, "large");
        assert_eq!((asset.width, asset.height), (1920, 620));
        std::fs::remove_dir_all(root).unwrap();
    }
}
