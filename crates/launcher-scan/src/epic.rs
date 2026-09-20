use crate::{
    DetectedGame, GameArt, GameArtCandidate, GameArtSource, LauncherKind, LauncherScanner,
    ScanError,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

pub struct EpicScanner;

impl LauncherScanner for EpicScanner {
    fn kind(&self) -> LauncherKind {
        LauncherKind::Epic
    }

    fn scan(&self) -> Result<Vec<DetectedGame>, ScanError> {
        let manifests_dir = match find_manifests_dir() {
            Some(p) => p,
            None => return Ok(Vec::new()),
        };
        let entries = match std::fs::read_dir(&manifests_dir) {
            Ok(d) => d,
            Err(_) => return Ok(Vec::new()),
        };
        let mut games = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_ascii_lowercase())
                != Some("item".to_string())
            {
                continue;
            }
            if let Some(game) = parse_manifest(&path) {
                games.push(game);
            }
        }
        Ok(games)
    }
}

fn find_manifests_dir() -> Option<PathBuf> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    for subkey in [
        "SOFTWARE\\WOW6432Node\\Epic Games\\EpicGamesLauncher",
        "SOFTWARE\\Epic Games\\EpicGamesLauncher",
    ] {
        if let Ok(k) = hklm.open_subkey(subkey) {
            if let Ok(path) = k.get_value::<String, _>("AppDataPath") {
                let dir = PathBuf::from(path).join("Manifests");
                if dir.exists() {
                    return Some(dir);
                }
            }
        }
    }
    let fallback = PathBuf::from("C:\\ProgramData\\Epic\\EpicGamesLauncher\\Data\\Manifests");
    if fallback.exists() {
        return Some(fallback);
    }
    None
}

fn parse_manifest(path: &std::path::Path) -> Option<DetectedGame> {
    let content = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    let install_location = v.get("InstallLocation")?.as_str()?.to_string();
    let install_dir = PathBuf::from(&install_location);
    if !install_dir.exists() {
        return None;
    }
    let app_name = v.get("AppName").and_then(|x| x.as_str()).unwrap_or("");
    let display = v
        .get("DisplayName")
        .and_then(|x| x.as_str())
        .unwrap_or(app_name)
        .to_string();
    let catalog_item_id = v
        .get("CatalogItemId")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let catalog_namespace = v
        .get("CatalogNamespace")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let vault_thumbnail = v
        .get("VaultThumbnailUrl")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim();
    let candidates = GameArtCandidate::https(
        vault_thumbnail,
        GameArtSource::EpicManifest,
        "vault_thumbnail",
        0,
        0,
    )
    .into_iter()
    .collect::<Vec<_>>();
    let art = if candidates.is_empty() {
        GameArt::unavailable(
            GameArtSource::EpicManifest,
            "no_local_or_official_cover_metadata",
        )
    } else {
        GameArt::pending(candidates)
    };
    let mut native_ids = BTreeMap::new();
    native_ids.insert("app_name".to_string(), app_name.to_string());
    if let Some(id) = catalog_item_id.as_ref() {
        native_ids.insert("catalog_item_id".to_string(), id.clone());
    }
    if !catalog_namespace.is_empty() {
        native_ids.insert("catalog_namespace".to_string(), catalog_namespace);
    }
    Some(DetectedGame {
        id: format!("epic-{}", app_name),
        name: display,
        launcher: LauncherKind::Epic,
        install_dir,
        app_id: catalog_item_id.or_else(|| Some(app_name.to_string())),
        native_ids,
        art,
        image_url: None,
        size_bytes: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GameArtState;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("dlssync-{name}-{nonce}"))
    }

    #[test]
    fn epic_manifest_preserves_native_ids_and_observed_cover_url() {
        let root = fixture_dir("epic-art");
        let install = root.join("game");
        std::fs::create_dir_all(&install).unwrap();
        let manifest = root.join("game.item");
        let json = serde_json::json!({
            "InstallLocation": install,
            "AppName": "app-name",
            "DisplayName": "Fixture Game",
            "CatalogNamespace": "namespace",
            "CatalogItemId": "catalog-id",
            "VaultThumbnailUrl": "https://cdn.example.invalid/cover.jpg"
        });
        std::fs::write(&manifest, serde_json::to_vec(&json).unwrap()).unwrap();

        let game = parse_manifest(&manifest).unwrap();
        assert_eq!(
            game.native_ids.get("catalog_item_id").map(String::as_str),
            Some("catalog-id")
        );
        assert_eq!(game.art.state, GameArtState::Pending);
        assert_eq!(game.art.candidates.len(), 1);
        assert_eq!(game.art.candidates[0].variant, "vault_thumbnail");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn epic_without_observed_cover_is_explicitly_unavailable() {
        let root = fixture_dir("epic-no-art");
        let install = root.join("game");
        std::fs::create_dir_all(&install).unwrap();
        let manifest = root.join("game.item");
        let json = serde_json::json!({
            "InstallLocation": install,
            "AppName": "modkit-app",
            "DisplayName": "inZOI ModKit",
            "CatalogNamespace": "namespace",
            "CatalogItemId": "catalog-id",
            "VaultThumbnailUrl": ""
        });
        std::fs::write(&manifest, serde_json::to_vec(&json).unwrap()).unwrap();

        let game = parse_manifest(&manifest).unwrap();
        assert_eq!(game.art.state, GameArtState::Unavailable);
        assert!(!game.art.retryable);
        assert_eq!(game.art.candidates.len(), 0);

        std::fs::remove_dir_all(root).unwrap();
    }
}
