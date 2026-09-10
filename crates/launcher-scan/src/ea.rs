use crate::{DetectedGame, LauncherKind, LauncherScanner, ScanError};
use std::path::PathBuf;
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
use winreg::RegKey;

pub struct EaDesktopScanner;

impl LauncherScanner for EaDesktopScanner {
    fn kind(&self) -> LauncherKind {
        LauncherKind::EaDesktop
    }

    fn scan(&self) -> Result<Vec<DetectedGame>, ScanError> {
        let uninstall = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey_with_flags(
                "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
                KEY_READ,
            )
            .or_else(|_| {
                RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(
                    "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
                    KEY_READ,
                )
            });
        let Ok(uninstall) = uninstall else {
            return Ok(Vec::new());
        };
        let mut games = Vec::new();
        for key_name in uninstall.enum_keys().flatten() {
            let Ok(key) = uninstall.open_subkey(&key_name) else {
                continue;
            };
            let publisher: String = key.get_value("Publisher").unwrap_or_default();
            let url: String = key.get_value("URLInfoAbout").unwrap_or_default();
            if !publisher.to_ascii_lowercase().contains("electronic arts")
                && !url.contains("ea.com")
            {
                continue;
            }
            let location: String = key.get_value("InstallLocation").unwrap_or_default();
            let install_dir = PathBuf::from(location.trim_matches('"'));
            if !install_dir.is_dir() {
                continue;
            }
            let name: String = key
                .get_value("DisplayName")
                .unwrap_or_else(|_| key_name.clone());
            games.push(DetectedGame {
                id: format!("ea-{key_name}"),
                name,
                launcher: LauncherKind::EaDesktop,
                install_dir,
                app_id: Some(key_name),
                image_url: None,
                size_bytes: None,
            });
        }
        Ok(games)
    }
}
