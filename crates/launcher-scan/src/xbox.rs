use crate::{DetectedGame, LauncherKind, LauncherScanner, ScanError};
use std::path::PathBuf;

pub struct XboxScanner;

impl LauncherScanner for XboxScanner {
    fn kind(&self) -> LauncherKind {
        LauncherKind::Xbox
    }
    fn scan(&self) -> Result<Vec<DetectedGame>, ScanError> {
        let mut roots = Vec::new();
        for drive in b'C'..=b'Z' {
            let root = PathBuf::from(format!("{}:\\XboxGames", drive as char));
            if root.is_dir() {
                roots.push(root);
            }
        }
        let mut games = Vec::new();
        for root in roots {
            let Ok(entries) = std::fs::read_dir(root) else {
                continue;
            };
            for entry in entries.flatten() {
                let content = entry.path().join("Content");
                if !content.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                games.push(DetectedGame {
                    id: format!("xbox-{}", name.to_ascii_lowercase().replace(' ', "-")),
                    name,
                    launcher: LauncherKind::Xbox,
                    install_dir: content,
                    app_id: None,
                    image_url: None,
                    size_bytes: None,
                });
            }
        }
        Ok(games)
    }
}
