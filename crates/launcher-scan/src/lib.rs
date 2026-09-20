use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub mod art;

pub use art::{
    ArtCacheStatus, ArtLocatorKind, ArtResolveTrigger, GameArt, GameArtAsset, GameArtCandidate,
    GameArtSource, GameArtState,
};

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("registry: {0}")]
    Registry(String),
    #[error("parse: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum LauncherKind {
    Steam,
    Epic,
    Gog,
    Ubisoft,
    EaDesktop,
    Xbox,
    Battlenet,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct DetectedGame {
    pub id: String,
    pub name: String,
    pub launcher: LauncherKind,
    pub install_dir: PathBuf,
    pub app_id: Option<String>,
    #[serde(default)]
    pub native_ids: BTreeMap<String, String>,
    #[serde(default)]
    pub art: GameArt,
    /// Compatibility projection for older frontend consumers. New code must use `art`.
    pub image_url: Option<String>,
    pub size_bytes: Option<u64>,
}

pub trait LauncherScanner {
    fn kind(&self) -> LauncherKind;
    fn scan(&self) -> Result<Vec<DetectedGame>, ScanError>;
}

#[cfg(windows)]
mod battlenet;
#[cfg(windows)]
mod ea;
#[cfg(windows)]
mod epic;
#[cfg(windows)]
mod gog;
mod steam;
#[cfg(windows)]
mod ubisoft;
#[cfg(windows)]
mod xbox;

#[cfg(windows)]
pub use battlenet::BattlenetScanner;
#[cfg(windows)]
pub use ea::EaDesktopScanner;
#[cfg(windows)]
pub use epic::EpicScanner;
#[cfg(windows)]
pub use gog::GogScanner;
pub use steam::SteamScanner;
#[cfg(windows)]
pub use ubisoft::UbisoftScanner;
#[cfg(windows)]
pub use xbox::XboxScanner;

pub fn scan_all(launchers: &[LauncherKind]) -> Result<Vec<DetectedGame>, ScanError> {
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        for kind in launchers {
            let result = match kind {
                LauncherKind::Steam => SteamScanner.scan(),
                LauncherKind::Epic => EpicScanner.scan(),
                LauncherKind::Gog => GogScanner.scan(),
                LauncherKind::Ubisoft => UbisoftScanner.scan(),
                LauncherKind::EaDesktop => EaDesktopScanner.scan(),
                LauncherKind::Xbox => XboxScanner.scan(),
                LauncherKind::Battlenet => BattlenetScanner.scan(),
                LauncherKind::Manual => Ok(Vec::new()),
            };
            match result {
                Ok(g) => out.extend(g),
                Err(e) => tracing::warn!(launcher = ?kind, error = %e, "launcher scan failed"),
            }
        }
    }
    #[cfg(not(windows))]
    {
        if launchers.contains(&LauncherKind::Steam) {
            match SteamScanner.scan() {
                Ok(games) => out.extend(games),
                Err(error) => tracing::warn!(%error, "Steam discovery failed"),
            }
        }
    }
    Ok(out)
}
