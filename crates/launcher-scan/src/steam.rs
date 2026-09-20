use crate::art::{best_local_asset, LocalVariant};
use crate::{DetectedGame, GameArt, GameArtSource, LauncherKind, LauncherScanner, ScanError};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

pub struct SteamScanner;

impl LauncherScanner for SteamScanner {
    fn kind(&self) -> LauncherKind {
        LauncherKind::Steam
    }

    fn scan(&self) -> Result<Vec<DetectedGame>, ScanError> {
        let steam_path = find_steam_install()?;
        scan_install(&steam_path)
    }
}

fn scan_install(steam_path: &Path) -> Result<Vec<DetectedGame>, ScanError> {
    let libraries = parse_library_folders(steam_path)?;
    let mut games = Vec::new();
    let mut failures = Vec::new();
    for lib in libraries {
        let apps_dir = lib.join("steamapps");
        let read_dir = match std::fs::read_dir(&apps_dir) {
            Ok(r) => r,
            Err(error) => {
                failures.push(format!("{}: {error}", apps_dir.display()));
                continue;
            }
        };
        for entry in read_dir {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    failures.push(error.to_string());
                    continue;
                }
            };
            let name = entry.file_name();
            let s = name.to_string_lossy();
            if !s.starts_with("appmanifest_") || !s.ends_with(".acf") {
                continue;
            }
            if let Some(game) = parse_appmanifest(&entry.path(), &apps_dir, steam_path) {
                games.push(game);
            }
        }
    }
    if failures.is_empty() {
        Ok(games)
    } else {
        Err(ScanError::Partial {
            games,
            detail: failures.join("; "),
        })
    }
}

fn find_steam_install() -> Result<PathBuf, ScanError> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    for subkey in [
        "SOFTWARE\\WOW6432Node\\Valve\\Steam",
        "SOFTWARE\\Valve\\Steam",
    ] {
        if let Ok(k) = hklm.open_subkey(subkey) {
            if let Ok(path) = k.get_value::<String, _>("InstallPath") {
                let p = PathBuf::from(path);
                if p.exists() {
                    return Ok(p);
                }
            }
        }
    }
    Err(ScanError::Registry("Steam install path not found".into()))
}

fn parse_library_folders(steam_path: &Path) -> Result<Vec<PathBuf>, ScanError> {
    let vdf_path = steam_path.join("steamapps").join("libraryfolders.vdf");
    let content = std::fs::read_to_string(&vdf_path)?;
    let mut libs = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("\"path\"") {
            let rest = rest.trim();
            if let Some(start) = rest.find('"') {
                let after = &rest[start + 1..];
                if let Some(end) = after.find('"') {
                    let raw = &after[..end];
                    let normalized = raw.replace("\\\\", "\\");
                    let p = PathBuf::from(normalized);
                    libs.push(p);
                }
            }
        }
    }
    if libs.is_empty() {
        libs.push(steam_path.to_path_buf());
    }
    Ok(libs)
}

const EXCLUDED_APPIDS: &[&str] = &[
    "228980", "1493710", "1391110", "1070560", "1628350", "228983",
];

const EXCLUDED_NAME_PREFIXES: &[&str] = &[
    "Steam Linux Runtime",
    "Proton ",
    "Proton-",
    "Proton Hotfix",
    "Steamworks ",
];

const STEAM_LANDSCAPE_VARIANTS: &[LocalVariant<'_>] = &[
    LocalVariant {
        file_name: "library_hero.jpg",
        variant: "library_hero",
        nominal_width: 3840,
        nominal_height: 1240,
    },
    LocalVariant {
        file_name: "library_header.jpg",
        variant: "library_header",
        nominal_width: 920,
        nominal_height: 430,
    },
    LocalVariant {
        file_name: "header.jpg",
        variant: "header",
        nominal_width: 460,
        nominal_height: 215,
    },
];

const STEAM_PORTRAIT_VARIANTS: &[LocalVariant<'_>] = &[
    LocalVariant {
        file_name: "library_600x900_2x.jpg",
        variant: "library_600x900_2x",
        nominal_width: 1200,
        nominal_height: 1800,
    },
    LocalVariant {
        file_name: "library_600x900.jpg",
        variant: "library_600x900",
        nominal_width: 600,
        nominal_height: 900,
    },
    LocalVariant {
        file_name: "library_capsule.jpg",
        variant: "library_capsule",
        nominal_width: 300,
        nominal_height: 450,
    },
];

fn parse_appmanifest(path: &Path, apps_dir: &Path, steam_path: &Path) -> Option<DetectedGame> {
    let content = std::fs::read_to_string(path).ok()?;
    let appid = extract_key(&content, "appid")?;
    if EXCLUDED_APPIDS.iter().any(|x| *x == appid) {
        return None;
    }
    let name = extract_key(&content, "name")?;
    if EXCLUDED_NAME_PREFIXES.iter().any(|p| name.starts_with(p)) {
        return None;
    }
    let installdir = extract_key(&content, "installdir")?;
    let size = extract_key(&content, "SizeOnDisk").and_then(|s| s.parse::<u64>().ok());
    let install_path = apps_dir.join("common").join(&installdir);
    if !install_path.exists() {
        return None;
    }
    let library_cache = steam_path
        .join("appcache")
        .join("librarycache")
        .join(&appid);
    let landscape = best_local_asset(
        &library_cache,
        STEAM_LANDSCAPE_VARIANTS,
        GameArtSource::SteamLibraryCache,
    );
    let portrait = best_local_asset(
        &library_cache,
        STEAM_PORTRAIT_VARIANTS,
        GameArtSource::SteamLibraryCache,
    );
    let art = if landscape.is_some() || portrait.is_some() {
        GameArt::resolved(landscape, portrait)
    } else {
        GameArt::pending(Vec::new())
    };
    let native_ids = BTreeMap::from([("app_id".to_string(), appid.clone())]);
    Some(DetectedGame {
        id: format!("steam-{}", appid),
        name,
        launcher: LauncherKind::Steam,
        install_dir: install_path,
        app_id: Some(appid),
        native_ids,
        art,
        image_url: None,
        size_bytes: size,
    })
}

fn extract_key(content: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let idx = content.find(&needle)?;
    let after = &content[idx + needle.len()..];
    let q1 = after.find('"')?;
    let after = &after[q1 + 1..];
    let q2 = after.find('"')?;
    Some(after[..q2].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn extracts_appmanifest_keys() {
        let acf = r#""AppState"
{
    "appid"        "440"
    "name"         "Team Fortress 2"
    "installdir"   "Team Fortress 2"
    "SizeOnDisk"   "17179869184"
}"#;
        assert_eq!(extract_key(acf, "appid"), Some("440".into()));
        assert_eq!(extract_key(acf, "name"), Some("Team Fortress 2".into()));
        assert_eq!(
            extract_key(acf, "installdir"),
            Some("Team Fortress 2".into())
        );
        assert_eq!(extract_key(acf, "SizeOnDisk"), Some("17179869184".into()));
    }

    #[test]
    fn launcher_appid_drives_verified_high_resolution_art() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("dlssync-steam-art-{nonce}"));
        let apps_dir = root.join("steamapps");
        let install_dir = apps_dir.join("common").join("How to Fish");
        std::fs::create_dir_all(&install_dir).unwrap();
        let manifest = apps_dir.join("appmanifest_4001890.acf");
        std::fs::write(
            &manifest,
            r#""AppState"
{
    "appid"        "4001890"
    "name"         "How to Fish"
    "installdir"   "How to Fish"
}"#,
        )
        .unwrap();

        let cache = root
            .join("appcache")
            .join("librarycache")
            .join("4001890")
            .join("hero-hash");
        std::fs::create_dir_all(&cache).unwrap();
        let mut png = vec![0_u8; 24];
        png[..8].copy_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
        png[16..20].copy_from_slice(&1920_u32.to_be_bytes());
        png[20..24].copy_from_slice(&620_u32.to_be_bytes());
        std::fs::write(cache.join("library_hero.jpg"), png).unwrap();

        let game = parse_appmanifest(&manifest, &apps_dir, &root).unwrap();
        assert_eq!(game.app_id.as_deref(), Some("4001890"));
        let landscape = game.art.landscape.as_ref().unwrap();
        assert_eq!(landscape.variant, "library_hero");
        assert_eq!((landscape.width, landscape.height), (1920, 620));
        assert!(landscape.verified);
        assert_eq!(game.image_url, None);

        std::fs::remove_dir_all(root).unwrap();
    }
}
