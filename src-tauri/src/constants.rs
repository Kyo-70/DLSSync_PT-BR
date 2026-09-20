pub const STEAM_CDN_BASE: &str = "https://cdn.cloudflare.steamstatic.com/steam/apps";
pub const STEAM_HEADER_PATH: &str = "header.jpg";
pub const STEAM_HERO_PATH: &str = "library_hero.jpg";
pub const STEAM_CAPSULE_2X_PATH: &str = "library_600x900_2x.jpg";
pub const STEAM_CAPSULE_PATH: &str = "library_600x900.jpg";

pub const SGDB_API_BASE: &str = "https://www.steamgriddb.com/api/v2";
pub const SGDB_GRID_DIMS: &str = "920x430,460x215";
pub const SGDB_HERO_DIMS: &str = "3840x2160,1920x620";

pub const ART_RESOLVED_CACHE_TTL_SECS: i64 = 30 * 24 * 60 * 60;
pub const ART_UNAVAILABLE_CACHE_TTL_SECS: i64 = 24 * 60 * 60;
pub const ART_PROTOCOL_CACHE_PARENT: &str = "game-art";
pub const ART_PROTOCOL_CACHE_DIR: &str = "asset-protocol";
pub const ART_PROTOCOL_MAX_ASSET_BYTES: u64 = 8 * 1024 * 1024;
pub const ART_PROTOCOL_CACHE_MAX_BYTES: u64 = 256 * 1024 * 1024;
pub const ART_PROTOCOL_CACHE_TTL_SECS: u64 = 30 * 24 * 60 * 60;
