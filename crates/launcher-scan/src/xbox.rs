use crate::art::{best_local_asset, LocalVariant};
use crate::{DetectedGame, GameArt, GameArtSource, LauncherKind, LauncherScanner, ScanError};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub struct XboxScanner;

const XBOX_LANDSCAPE_VARIANTS: &[LocalVariant<'_>] = &[
    LocalVariant {
        file_name: "Hero.png",
        variant: "hero",
        nominal_width: 0,
        nominal_height: 0,
    },
    LocalVariant {
        file_name: "SplashScreen.png",
        variant: "splash_screen",
        nominal_width: 0,
        nominal_height: 0,
    },
    LocalVariant {
        file_name: "Cover.jpg",
        variant: "cover",
        nominal_width: 0,
        nominal_height: 0,
    },
    LocalVariant {
        file_name: "Cover.png",
        variant: "cover",
        nominal_width: 0,
        nominal_height: 0,
    },
];

const XBOX_PORTRAIT_VARIANTS: &[LocalVariant<'_>] = &[LocalVariant {
    file_name: "Poster.png",
    variant: "poster",
    nominal_width: 0,
    nominal_height: 0,
}];

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
                let landscape = best_local_asset(
                    &content,
                    XBOX_LANDSCAPE_VARIANTS,
                    GameArtSource::XboxPackage,
                );
                let portrait =
                    best_local_asset(&content, XBOX_PORTRAIT_VARIANTS, GameArtSource::XboxPackage);
                let art = if landscape.is_some() || portrait.is_some() {
                    GameArt::resolved(landscape, portrait)
                } else {
                    GameArt::unavailable(GameArtSource::XboxPackage, "no_local_cover_asset")
                };
                games.push(DetectedGame {
                    id: format!("xbox-{}", name.to_ascii_lowercase().replace(' ', "-")),
                    name: name.clone(),
                    launcher: LauncherKind::Xbox,
                    install_dir: content,
                    app_id: Some(name.clone()),
                    native_ids: BTreeMap::from([("package_folder".to_string(), name.clone())]),
                    art,
                    image_url: None,
                    size_bytes: None,
                });
            }
        }
        Ok(games)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GameArtState;

    #[test]
    fn xbox_package_uses_verified_local_landscape_art() {
        let root = std::env::temp_dir().join("dlssync-xbox-art-fixture");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut png = vec![0_u8; 24];
        png[..8].copy_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
        png[16..20].copy_from_slice(&1920_u32.to_be_bytes());
        png[20..24].copy_from_slice(&1080_u32.to_be_bytes());
        std::fs::write(root.join("SplashScreen.png"), png).unwrap();

        let landscape =
            best_local_asset(&root, XBOX_LANDSCAPE_VARIANTS, GameArtSource::XboxPackage);
        let art = GameArt::resolved(landscape, None);
        assert_eq!(art.state, GameArtState::Resolved);
        assert_eq!(art.landscape.as_ref().unwrap().variant, "splash_screen");
        assert_eq!(
            (
                art.landscape.as_ref().unwrap().width,
                art.landscape.as_ref().unwrap().height
            ),
            (1920, 1080)
        );

        std::fs::remove_dir_all(root).unwrap();
    }
}
