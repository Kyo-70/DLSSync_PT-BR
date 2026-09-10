use crate::{DetectedGame, LauncherKind, LauncherScanner, ScanError};
use serde::Deserialize;
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
            install_dir.is_dir().then(|| DetectedGame {
                id: format!("battlenet-{}", product.uid),
                name: if product.product_name.is_empty() {
                    product.uid.clone()
                } else {
                    product.product_name
                },
                launcher: LauncherKind::Battlenet,
                install_dir,
                app_id: Some(product.uid),
                image_url: None,
                size_bytes: None,
            })
        })
        .collect())
}
