use crate::art::verified_dimensions;
use crate::{
    DetectedGame, GameArt, GameArtAsset, GameArtSource, LauncherKind, LauncherScanner, ScanError,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct BattlenetScanner;

#[derive(Deserialize, specta::Type)]
struct ProductDb {
    #[serde(default)]
    products: Vec<Product>,
}
#[derive(Deserialize, specta::Type)]
struct Product {
    uid: String,
    #[serde(default)]
    product_name: String,
    #[serde(default)]
    install_path: String,
    #[serde(default, alias = "coverPath")]
    cover_path: String,
    #[serde(default, alias = "heroPath")]
    hero_path: String,
}

impl LauncherScanner for BattlenetScanner {
    fn kind(&self) -> LauncherKind {
        LauncherKind::Battlenet
    }
    fn scan(&self) -> Result<Vec<DetectedGame>, ScanError> {
        let Some(program_data) = std::env::var_os("PROGRAMDATA") else {
            return Ok(Vec::new());
        };
        parse_product_db(&PathBuf::from(program_data).join("Battle.net\\Agent\\product.db"))
    }
}

fn parse_product_db(path: &Path) -> Result<Vec<DetectedGame>, ScanError> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Ok(Vec::new());
    };
    let db: ProductDb = serde_json::from_str(&raw).map_err(|e| ScanError::Parse(e.to_string()))?;
    Ok(db
        .products
        .into_iter()
        .filter_map(|product| {
            let install_dir = PathBuf::from(&product.install_path);
            let art = battlenet_art(&product);
            install_dir.is_dir().then(|| DetectedGame {
                id: format!("battlenet-{}", product.uid),
                name: if product.product_name.is_empty() {
                    product.uid.clone()
                } else {
                    product.product_name
                },
                launcher: LauncherKind::Battlenet,
                install_dir,
                app_id: Some(product.uid.clone()),
                native_ids: BTreeMap::from([("product_uid".to_string(), product.uid.clone())]),
                art,
                image_url: None,
                size_bytes: None,
            })
        })
        .collect())
}

fn battlenet_art(product: &Product) -> GameArt {
    let landscape = explicit_local_asset(&product.hero_path, "hero");
    let portrait = explicit_local_asset(&product.cover_path, "cover");
    if landscape.is_some() || portrait.is_some() {
        GameArt::resolved(landscape, portrait)
    } else {
        GameArt::unavailable(GameArtSource::BattlenetProductDb, "no_local_cover_metadata")
    }
}

fn explicit_local_asset(path: &str, variant: &str) -> Option<GameArtAsset> {
    let path = Path::new(path);
    let (width, height) = verified_dimensions(path)?;
    Some(GameArtAsset::local(
        path,
        GameArtSource::BattlenetProductDb,
        variant,
        width,
        height,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GameArtState;

    #[test]
    fn battlenet_without_product_art_is_explicitly_unavailable() {
        let product = Product {
            uid: "fixture".into(),
            product_name: "Fixture".into(),
            install_path: String::new(),
            cover_path: String::new(),
            hero_path: String::new(),
        };
        let art = battlenet_art(&product);
        assert_eq!(art.state, GameArtState::Unavailable);
        assert!(!art.retryable);
    }
}
