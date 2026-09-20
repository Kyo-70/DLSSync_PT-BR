use crate::{DetectedGame, GameArt, GameArtSource, LauncherKind, LauncherScanner, ScanError};
use std::collections::BTreeMap;
use std::path::PathBuf;
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

pub struct UbisoftScanner;

impl LauncherScanner for UbisoftScanner {
    fn kind(&self) -> LauncherKind {
        LauncherKind::Ubisoft
    }

    fn scan(&self) -> Result<Vec<DetectedGame>, ScanError> {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let root = match hklm.open_subkey("SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs") {
            Ok(r) => r,
            Err(_) => return Ok(Vec::new()),
        };
        let mut games = Vec::new();
        for id in root.enum_keys().flatten() {
            if let Ok(k) = root.open_subkey(&id) {
                let dir: String = k.get_value("InstallDir").unwrap_or_default();
                let p = PathBuf::from(&dir);
                if !p.exists() {
                    continue;
                }
                let display = hklm
                    .open_subkey(format!(
                        "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Uplay Install {}",
                        id
                    ))
                    .and_then(|u| u.get_value::<String, _>("DisplayName"))
                    .unwrap_or_else(|_| id.clone());
                games.push(DetectedGame {
                    id: format!("ubisoft-{}", id),
                    name: display,
                    launcher: LauncherKind::Ubisoft,
                    install_dir: p,
                    app_id: Some(id.clone()),
                    native_ids: BTreeMap::from([("install_id".to_string(), id.clone())]),
                    art: launcher_art(),
                    image_url: None,
                    size_bytes: None,
                });
            }
        }
        Ok(games)
    }
}

fn launcher_art() -> GameArt {
    GameArt::unavailable(GameArtSource::UbisoftRegistry, "no_cover_metadata")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GameArtState;

    #[test]
    fn ubisoft_registry_without_cover_metadata_is_explicitly_unavailable() {
        let art = launcher_art();
        assert_eq!(art.state, GameArtState::Unavailable);
        assert!(!art.retryable);
    }
}
