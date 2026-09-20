use crate::constants::{
    ART_PROTOCOL_MAX_ASSET_BYTES, ART_RESOLVED_CACHE_TTL_SECS, ART_UNAVAILABLE_CACHE_TTL_SECS,
    SGDB_API_BASE, SGDB_GRID_DIMS, SGDB_HERO_DIMS, STEAM_CAPSULE_2X_PATH, STEAM_CAPSULE_PATH,
    STEAM_CDN_BASE, STEAM_HEADER_PATH, STEAM_HERO_PATH,
};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use dlssync_contracts::{
    DistributionChannel, OperationActor, OperationKind, OperationRecord, OperationStatus,
};
use launcher_scan::{
    ArtCacheStatus, ArtLocatorKind, ArtResolveTrigger, DetectedGame, GameArt, GameArtAsset,
    GameArtCandidate, GameArtSource, GameArtState, LauncherKind, LauncherScanner,
};
use once_cell::sync::Lazy;
use operation_journal::{JournalError, JournalStore};
use std::collections::{BTreeMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tauri::State;

struct ScanDiscovery {
    games: Vec<DetectedGame>,
    successful_launchers: HashSet<LauncherKind>,
    successful_custom_roots: Vec<PathBuf>,
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn scan_libraries(
    state: State<'_, AppState>,
    launchers: Vec<LauncherKind>,
) -> AppResult<Vec<DetectedGame>> {
    let launchers = effective_launchers(launchers, e2e_mode_enabled());
    let launcher_count = launchers.len();
    let (overrides, custom_folders) = {
        let s = state.settings.read();
        (
            s.launcher_overrides.clone(),
            s.launcher_overrides.custom.clone(),
        )
    };

    let started = Instant::now();
    let mut result = tokio::task::spawn_blocking(move || {
        let mut all = Vec::new();
        let mut successful_launchers = HashSet::new();
        let mut successful_custom_roots = Vec::new();

        for launcher in launchers {
            match scan_launcher(launcher) {
                Ok(games) => {
                    successful_launchers.insert(launcher);
                    all.extend(games);
                }
                Err(error) => {
                    tracing::warn!(launcher = ?launcher, %error, "launcher scan failed");
                }
            }
        }

        for folder in custom_folders {
            let path = PathBuf::from(&folder);
            if let Some(games) = scan_custom_folder(&path) {
                successful_custom_roots.push(path);
                all.extend(games);
            }
        }

        if overrides.steam.iter().any(|p| !p.is_empty()) {
            for extra in &overrides.steam {
                let p = PathBuf::from(extra);
                if let Some(games) = scan_custom_folder(&p) {
                    successful_custom_roots.push(p);
                    all.extend(games);
                }
            }
        }

        ScanDiscovery {
            games: deduplicate(all),
            successful_launchers,
            successful_custom_roots,
        }
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()));

    if let Ok(discovery) = result.as_mut() {
        let games = &mut discovery.games;
        let cache_dir = app_cache_dir(&state)?;
        let mut transport = crate::art_transport::ArtTransportSession::new(&cache_dir)
            .map_err(|error| AppError::Other(format!("initialize game art transport: {error}")))?;
        for game in games.iter_mut() {
            if let Err(error) = transport.prepare(&mut game.art) {
                let source = game
                    .art
                    .preferred()
                    .map(|asset| asset.source)
                    .unwrap_or(GameArtSource::ManualFolder);
                tracing::warn!(game_id = %game.id, %error, "game art transport preparation failed");
                game.art = GameArt::source_failed(source, "asset_transport_failed");
                game.art.cache_status = ArtCacheStatus::TransientFailure;
            }
        }
        transport
            .finish()
            .map_err(|error| AppError::Other(format!("clean game art transport cache: {error}")))?;

        let observed = games
            .iter()
            .map(|game| {
                dlssync_application::scan::observe_game_snapshot(
                    game,
                    None,
                    &dlssync_application::scan::ProjectionSettings::default(),
                )
            })
            .collect::<Vec<_>>();
        // A scan may overlap with the Library mount scan. Take the projection lock before
        // capturing the previous snapshot and the observation ticket, so the commit cannot be
        // invalidated by another scan that acquired a generation between discovery and publish.
        let _projection_guard = dlssync_application::scan::projection_guard();
        let ticket = state
            .authoritative_state
            .begin_observation(dlssync_application::scan::GAME_PROJECTION_SCOPE);
        let catalog = state.catalog.read().clone();
        let settings = state.settings.read().clone();
        let snapshots = observed
            .iter()
            .map(|snapshot| {
                dlssync_application::scan::reproject_game_snapshot(
                    snapshot,
                    catalog.as_ref(),
                    &crate::commands::settings::projection_settings(&settings, &snapshot.id),
                )
            })
            .collect::<Vec<_>>();
        let previous = state.authoritative_state.snapshot();
        let observed_ids = snapshots
            .iter()
            .map(|snapshot| snapshot.id.as_str())
            .collect::<HashSet<_>>();
        let removed_game_ids = removed_games_for_successful_scopes(
            &previous.games,
            &observed_ids,
            &discovery.successful_launchers,
            &discovery.successful_custom_roots,
        );
        let mut affected_game_ids = snapshots
            .iter()
            .map(|snapshot| snapshot.id.clone())
            .collect::<Vec<_>>();
        affected_game_ids.extend(removed_game_ids.iter().cloned());
        if !snapshots.is_empty() || !removed_game_ids.is_empty() {
            match state.authoritative_state.commit_observation(
                ticket,
                dlssync_application::state::StateCommit {
                    delta: dlssync_contracts::StateDelta {
                        affected_game_ids,
                        games: snapshots,
                        removed_game_ids,
                        ..dlssync_contracts::StateDelta::default()
                    },
                    ..dlssync_application::state::StateCommit::default()
                },
            ) {
                Ok(receipt) => {
                    if let Some(error) = receipt.delivery_error {
                        tracing::warn!(%error, "game observation event delivery failed");
                    }
                }
                Err(error) => tracing::warn!(%error, "library observation commit was stale"),
            }
        }
        if let Err(error) = crate::commands::runtime::refresh_persisted_state(state.inner()) {
            tracing::warn!(%error, "persisted-state refresh after scan failed");
        }
    }

    let duration_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    let outcome = match &result {
        Ok(discovery) => Ok(discovery.games.len()),
        Err(err) => Err(err.to_string()),
    };
    if let Some(journal) = state.journal.read().as_ref() {
        if let Err(err) = record_scan(journal, launcher_count, duration_ms, outcome) {
            tracing::warn!(error = %err, "failed to journal library scan");
        }
    }

    result.map(|discovery| discovery.games)
}

#[cfg(windows)]
fn scan_launcher(kind: LauncherKind) -> Result<Vec<DetectedGame>, launcher_scan::ScanError> {
    match kind {
        LauncherKind::Steam => launcher_scan::SteamScanner.scan(),
        LauncherKind::Epic => launcher_scan::EpicScanner.scan(),
        LauncherKind::Gog => launcher_scan::GogScanner.scan(),
        LauncherKind::Ubisoft => launcher_scan::UbisoftScanner.scan(),
        LauncherKind::EaDesktop => launcher_scan::EaDesktopScanner.scan(),
        LauncherKind::Xbox => launcher_scan::XboxScanner.scan(),
        LauncherKind::Battlenet => launcher_scan::BattlenetScanner.scan(),
        LauncherKind::Manual => Ok(Vec::new()),
    }
}

#[cfg(not(windows))]
fn scan_launcher(_kind: LauncherKind) -> Result<Vec<DetectedGame>, launcher_scan::ScanError> {
    Err(launcher_scan::ScanError::Parse(
        "launcher discovery is available only on Windows".into(),
    ))
}

fn removed_games_for_successful_scopes(
    existing: &[dlssync_contracts::GameSnapshot],
    observed_ids: &HashSet<&str>,
    successful_launchers: &HashSet<LauncherKind>,
    successful_custom_roots: &[PathBuf],
) -> Vec<String> {
    existing
        .iter()
        .filter(|game| !observed_ids.contains(game.id.as_str()))
        .filter(|game| {
            let launcher_success = game
                .launcher
                .as_deref()
                .and_then(|launcher| {
                    serde_json::from_value::<LauncherKind>(serde_json::Value::String(
                        launcher.to_string(),
                    ))
                    .ok()
                })
                .is_some_and(|launcher| successful_launchers.contains(&launcher));
            let custom_scope_success = game.launcher.as_deref() == Some("manual")
                && Path::new(&game.install_dir).parent().is_some_and(|parent| {
                    successful_custom_roots.iter().any(|root| {
                        normalize_path_identity(parent) == normalize_path_identity(root)
                    })
                });
            launcher_success || custom_scope_success
        })
        .map(|game| game.id.clone())
        .collect()
}

/// Write one `OperationKind::Scan` journal record for a library scan. Success
/// carries the detected-game count; failure carries an actionable message. The
/// target is always `None` — a scan's journal entry never records Tony's paths or
/// private library names (the app-wide redaction guard handles any stray detail).
fn record_scan(
    journal: &JournalStore,
    launcher_count: usize,
    duration_ms: u32,
    outcome: Result<usize, String>,
) -> Result<(), JournalError> {
    let (status, summary, games_detected, error) = match outcome {
        Ok(count) => (
            OperationStatus::Succeeded,
            "Library scan completed",
            count,
            None,
        ),
        Err(message) => (
            OperationStatus::Failed,
            "Library scan failed",
            0,
            Some(message),
        ),
    };
    let details = BTreeMap::from([
        ("games_detected".to_string(), games_detected.to_string()),
        ("launchers_scanned".to_string(), launcher_count.to_string()),
    ]);
    journal.append(&OperationRecord {
        id: uuid::Uuid::new_v4().to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        actor: OperationActor::Gui,
        kind: OperationKind::Scan,
        status,
        target: None,
        summary: summary.to_string(),
        details,
        duration_ms: Some(duration_ms),
        backup_id: None,
        error,
    })
}

fn effective_launchers(launchers: Vec<LauncherKind>, e2e_mode: bool) -> Vec<LauncherKind> {
    if e2e_mode {
        Vec::new()
    } else {
        launchers
    }
}

#[cfg(debug_assertions)]
fn e2e_mode_enabled() -> bool {
    std::env::var_os("DLSSYNC_E2E").as_deref() == Some(std::ffi::OsStr::new("1"))
}

#[cfg(not(debug_assertions))]
fn e2e_mode_enabled() -> bool {
    false
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn detect_dlls(
    _state: State<'_, AppState>,
    install_dir: String,
) -> AppResult<Vec<dll_scanner::DllRecord>> {
    let path = PathBuf::from(install_dir);
    crate::paths::PathGuard::assert_safe_scan_dir(&path)
        .map_err(|e| AppError::Validation(e.to_string()))?;
    let records = tokio::task::spawn_blocking(move || dll_scanner::scan_install(&path))
        .await
        .map_err(|e| AppError::Other(e.to_string()))??;
    Ok(records)
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn detect_dlss_enabler(
    _state: State<'_, AppState>,
    install_dir: String,
) -> AppResult<bool> {
    let path = PathBuf::from(install_dir);
    let present = tokio::task::spawn_blocking(move || dll_scanner::detect_dlss_enabler(&path))
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;
    Ok(present)
}

fn deduplicate(games: Vec<DetectedGame>) -> Vec<DetectedGame> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(games.len());
    for g in games {
        if seen.insert(g.install_dir.clone()) {
            out.push(g);
        }
    }
    out
}

#[cfg(test)]
mod e2e_isolation_tests {
    use super::*;
    use dlssync_contracts::{GameSnapshot, JournalFilter};

    fn snapshot(id: &str, launcher: &str, install_dir: &str) -> GameSnapshot {
        GameSnapshot {
            id: id.to_string(),
            name: id.to_string(),
            install_dir: install_dir.to_string(),
            launcher: Some(launcher.to_string()),
            ..GameSnapshot::default()
        }
    }

    #[test]
    fn e2e_mode_disables_host_launcher_discovery() {
        let launchers = vec![LauncherKind::Steam, LauncherKind::Epic];

        assert!(effective_launchers(launchers, true).is_empty());
    }

    #[test]
    fn normal_mode_preserves_requested_launchers() {
        let launchers = vec![LauncherKind::Steam, LauncherKind::Epic];

        assert_eq!(effective_launchers(launchers.clone(), false), launchers);
    }

    #[test]
    fn custom_game_ids_are_distinct_under_a_long_common_root() {
        let root = Path::new(
            r"C:\Users\someone\AppData\Local\Temp\dlssync-e2e-abcdef1234567890\FixtureGames",
        );
        let a = custom_game_id(&root.join("Aurora Protocol"), "Aurora Protocol");
        let b = custom_game_id(&root.join("Neon Divide"), "Neon Divide");
        assert_ne!(
            a, b,
            "distinct games under a long common root must get distinct ids (both were {a})"
        );
    }

    #[test]
    fn custom_game_id_is_stable_and_deterministic() {
        let dir = Path::new(r"C:\Games\Neon Divide");
        assert_eq!(
            custom_game_id(dir, "Neon Divide"),
            custom_game_id(dir, "Neon Divide")
        );
    }

    #[test]
    fn custom_game_id_is_readable_and_path_sensitive() {
        let id = custom_game_id(Path::new(r"C:\Games\Neon Divide"), "Neon Divide");
        assert!(
            id.starts_with("custom-"),
            "id must keep the custom- prefix: {id}"
        );
        assert!(
            id.to_lowercase().contains("neon"),
            "id must carry a readable name slug: {id}"
        );
        // Same name in a different folder is a different game -> different id.
        let other = custom_game_id(Path::new(r"D:\Library\Neon Divide"), "Neon Divide");
        assert_ne!(id, other);
    }

    #[test]
    fn custom_game_id_normalizes_windows_path_identity() {
        // Case + separator differences describe the same Windows path -> same id.
        let a = custom_game_id(Path::new(r"C:\Games\Neon Divide"), "Neon Divide");
        let b = custom_game_id(Path::new("c:/games/neon divide"), "Neon Divide");
        if cfg!(windows) {
            assert_eq!(
                a, b,
                "windows path identity must be case/separator-insensitive"
            );
        }
    }

    #[test]
    fn successful_scan_writes_one_gui_scan_journal_record() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JournalStore::open(dir.path().join("journal.db")).unwrap();
        record_scan(&journal, 2, 123, Ok(4)).unwrap();
        let rows = journal.list(&JournalFilter::default()).unwrap();
        assert_eq!(rows.len(), 1);
        let rec = &rows[0];
        assert_eq!(rec.kind, OperationKind::Scan);
        assert_eq!(rec.actor, OperationActor::Gui);
        assert_eq!(rec.status, OperationStatus::Succeeded);
        assert_eq!(rec.duration_ms, Some(123));
        assert_eq!(
            rec.details.get("games_detected").map(String::as_str),
            Some("4")
        );
        assert!(
            rec.target.is_none(),
            "scan journal must never carry a path or library-name target"
        );
    }

    #[test]
    fn failed_scan_records_failure_without_leaking_paths() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JournalStore::open(dir.path().join("journal.db")).unwrap();
        record_scan(&journal, 1, 5, Err("scan worker crashed".to_string())).unwrap();
        let rows = journal.list(&JournalFilter::default()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, OperationStatus::Failed);
        assert_eq!(rows[0].error.as_deref(), Some("scan worker crashed"));
        assert!(rows[0].target.is_none());
        let export = journal
            .export_redacted_json(&JournalFilter::default())
            .unwrap();
        assert!(
            !export.contains(":\\"),
            "redacted export must not leak windows paths"
        );
    }

    #[test]
    fn completed_launcher_scope_removes_only_missing_games_from_that_scope() {
        let existing = vec![
            snapshot("steam-old", "steam", r"C:\Steam\steam-old"),
            snapshot("epic-old", "epic", r"C:\Epic\epic-old"),
        ];
        let observed = HashSet::new();
        let successful = HashSet::from([LauncherKind::Steam]);

        let removed = removed_games_for_successful_scopes(&existing, &observed, &successful, &[]);

        assert_eq!(removed, vec!["steam-old"]);
    }

    #[test]
    fn observed_game_is_never_removed_from_a_completed_scope() {
        let existing = vec![snapshot("steam-present", "steam", r"C:\Steam\present")];
        let observed = HashSet::from(["steam-present"]);
        let successful = HashSet::from([LauncherKind::Steam]);

        let removed = removed_games_for_successful_scopes(&existing, &observed, &successful, &[]);

        assert!(removed.is_empty());
    }

    #[test]
    fn failed_launcher_scope_preserves_previous_membership() {
        let existing = vec![snapshot("steam-old", "steam", r"C:\Steam\steam-old")];

        let removed =
            removed_games_for_successful_scopes(&existing, &HashSet::new(), &HashSet::new(), &[]);

        assert!(removed.is_empty());
    }

    #[test]
    fn completed_custom_root_removes_only_its_missing_manual_children() {
        let existing = vec![
            snapshot("custom-a", "manual", r"C:\Games\Custom\Game A"),
            snapshot("custom-b", "manual", r"D:\Other\Game B"),
        ];

        let removed = removed_games_for_successful_scopes(
            &existing,
            &HashSet::new(),
            &HashSet::new(),
            &[PathBuf::from(r"C:\Games\Custom")],
        );

        assert_eq!(removed, vec!["custom-a"]);
    }
}

fn scan_custom_folder(root: &Path) -> Option<Vec<DetectedGame>> {
    if !root.exists() {
        return None;
    }
    let mut games = Vec::new();
    let read = match std::fs::read_dir(root) {
        Ok(r) => r,
        Err(_) => return None,
    };
    for entry in read.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        if !is_likely_game(&path, &name) {
            tracing::debug!(folder = %name, "skipped custom folder — not a game");
            continue;
        }
        let id = custom_game_id(&path, &name);
        games.push(DetectedGame {
            id,
            name,
            launcher: LauncherKind::Manual,
            install_dir: path,
            app_id: None,
            native_ids: BTreeMap::new(),
            art: GameArt::unavailable(GameArtSource::ManualFolder, "no_launcher_cover_source"),
            image_url: None,
            size_bytes: None,
        });
    }
    Some(games)
}

/// Stable, collision-free id for a custom-folder game. The game name provides a
/// readable, bounded slug; the full install path — normalized for Windows
/// case/separator identity — provides a SHA-256 suffix so two games under the
/// same long root can never collapse to one id. Deterministic across processes
/// (unlike a randomized `DefaultHasher`).
fn custom_game_id(install_dir: &Path, name: &str) -> String {
    format!(
        "custom-{}-{}",
        name_slug(name),
        stable_path_hash(install_dir)
    )
}

/// Readable, bounded ascii kebab slug of a game name; empty input -> "game".
fn name_slug(name: &str) -> String {
    let mut slug = String::with_capacity(40);
    let mut pending_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(ch.to_ascii_lowercase());
            if slug.len() >= 40 {
                break;
            }
        } else {
            pending_dash = true;
        }
    }
    if slug.is_empty() {
        "game".to_string()
    } else {
        slug
    }
}

/// Case/separator-normalized string identity of a path. Windows paths are
/// case-insensitive and accept either separator, so the same location always
/// hashes to the same value.
fn normalize_path_identity(path: &Path) -> String {
    let unified = path.to_string_lossy().replace('/', "\\");
    if cfg!(windows) {
        unified.to_lowercase()
    } else {
        unified
    }
}

/// First 12 hex chars of the SHA-256 of the normalized path — deterministic and
/// stable across runs.
fn stable_path_hash(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(normalize_path_identity(path).as_bytes());
    digest[..6].iter().map(|b| format!("{:02x}", b)).collect()
}

static EXCLUDED_FOLDER_NAMES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "saves",
        "save",
        "savegames",
        "saved games",
        "save games",
        "backup",
        "backups",
        "backup_old",
        "old",
        "archive",
        "archived",
        "cache",
        "caches",
        "cached",
        "logs",
        "log",
        "crashes",
        "crash",
        "temp",
        "tmp",
        "trash",
        "recycle",
        "downloads",
        "download",
        "documents",
        "pictures",
        "tools",
        "tool",
        "utility",
        "utilities",
        "utils",
        "mods",
        "mod",
        "patches",
        "patch",
        "addons",
        "plugins",
        "extracted",
        "extracts",
        "unpacked",
        "build",
        "builds",
        "output",
        "obj",
        "bin",
        "configs",
        "config",
        "configuration",
        "settings",
        "resources",
        "assets",
        "data",
        "workspace",
        "scratch",
        "test",
        "tests",
        "source",
        "src",
        "sources",
        "docs",
        "documentation",
        "common",
        "shared",
        ".cache",
        ".config",
        ".git",
        ".vs",
        ".idea",
        "node_modules",
        "__pycache__",
        "venv",
        "system volume information",
        "$recycle.bin",
    ]
    .into_iter()
    .collect()
});

static EXCLUDED_MODDING_TOOLS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "xedit",
        "fo4edit",
        "fnvedit",
        "fo3edit",
        "sseedit",
        "tes5edit",
        "tes4edit",
        "enderaledit",
        "f76edit",
        "starfieldedit",
        "zedit",
        "fomm",
        "fomm-le",
        "mo",
        "mo2",
        "mod organizer",
        "mod organizer 2",
        "wrye",
        "wrye bash",
        "wrye flash",
        "wrye smash",
        "wrye mash",
        "loot",
        "vortex",
        "nmm",
        "nexus mod manager",
        "bodyslide",
        "bs",
        "bodyslide and outfit studio",
        "outfitstudio",
        "obse",
        "skse",
        "skse64",
        "f4se",
        "mwse",
        "nvse",
        "fose",
        "starfieldse",
        "nemesis",
        "fnis",
        "cathedral assets optimizer",
        "cao",
        "bsa browser",
        "bsa unpacker",
        "bae",
        "xlodgen",
        "dyndolod",
        "tes5lodgen",
        "synthesis",
        "mator smash",
        "creation kit",
        "ck",
        "geck",
        "enbseries",
        "enb",
        "enboost",
        "reshade",
        "reshade-shaders",
        "spriggit",
        "fomod",
        "ddsopt",
        "uvtools",
        "zlibmod",
    ]
    .into_iter()
    .collect()
});

#[derive(Default)]
struct FolderMarkers {
    has_engine_marker: bool,
    max_exe_mb: u64,
}

fn is_likely_game(path: &Path, folder_name: &str) -> bool {
    if folder_name.len() < 3 {
        return false;
    }
    let lower = folder_name.to_lowercase();
    if EXCLUDED_FOLDER_NAMES.contains(lower.as_str()) {
        return false;
    }
    if EXCLUDED_MODDING_TOOLS.contains(lower.as_str()) {
        return false;
    }
    if lower.starts_with('.') || lower.starts_with('$') || lower.starts_with('_') {
        return false;
    }
    if lower.starts_with("backup") || lower.starts_with("temp") || lower.ends_with("-backup") {
        return false;
    }

    let m = scan_folder_markers(path, 0);
    m.has_engine_marker || m.max_exe_mb >= 5
}

fn scan_folder_markers(path: &Path, depth: u8) -> FolderMarkers {
    let mut m = FolderMarkers::default();
    if depth > 2 {
        return m;
    }
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return m,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let name_lower = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        if p.is_file() {
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if ext == "exe" {
                if let Ok(meta) = entry.metadata() {
                    let mb = meta.len() / (1024 * 1024);
                    if mb > m.max_exe_mb {
                        m.max_exe_mb = mb;
                    }
                }
            }
            if ext == "uproject" || ext == "uplugin" || name_lower == "unityplayer.dll" {
                m.has_engine_marker = true;
            }
        } else if p.is_dir() {
            if name_lower == "binaries"
                || name_lower == "bin64"
                || name_lower == "engine"
                || name_lower == "content"
                || name_lower == "paks"
                || name_lower.ends_with("_data")
            {
                m.has_engine_marker = true;
            }
            if depth < 2
                && (name_lower == "binaries"
                    || name_lower == "bin64"
                    || name_lower == "bin"
                    || name_lower == "win64"
                    || name_lower == "x64"
                    || name_lower == "game"
                    || name_lower == "engine")
            {
                let nested = scan_folder_markers(&p, depth + 1);
                if nested.max_exe_mb > m.max_exe_mb {
                    m.max_exe_mb = nested.max_exe_mb;
                }
                if nested.has_engine_marker {
                    m.has_engine_marker = true;
                }
            }
        }
    }
    m
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ArtCacheEntry {
    expires_at: i64,
    art: GameArt,
}

#[derive(Debug)]
enum RemoteProbe {
    Found(Vec<u8>),
    Missing,
    Failed,
}

#[derive(Debug)]
struct RemoteFound {
    candidate: GameArtCandidate,
    bytes: Vec<u8>,
    width: u32,
    height: u32,
}

fn remote_art_allowed(channel: DistributionChannel, trigger: ArtResolveTrigger) -> bool {
    match (channel, trigger) {
        (_, ArtResolveTrigger::Automatic) => false,
        (DistributionChannel::Nexus, ArtResolveTrigger::UserScan)
        | (DistributionChannel::Nexus, ArtResolveTrigger::ExplicitRetry)
        | (DistributionChannel::Standard, ArtResolveTrigger::UserScan)
        | (DistributionChannel::Standard, ArtResolveTrigger::ExplicitRetry) => true,
    }
}

fn policy_pending() -> GameArt {
    let mut art = GameArt::pending(Vec::new());
    art.error_code = Some("remote_policy:explicit_action_required".to_string());
    art
}

fn steam_landscape_candidates(app_id: &str) -> Vec<GameArtCandidate> {
    [
        (STEAM_HERO_PATH, "library_hero", 3840, 1240),
        (STEAM_HEADER_PATH, "header", 920, 430),
    ]
    .into_iter()
    .filter_map(|(path, variant, width, height)| {
        GameArtCandidate::https(
            format!("{STEAM_CDN_BASE}/{app_id}/{path}"),
            GameArtSource::SteamOfficialCdn,
            variant,
            width,
            height,
        )
    })
    .collect()
}

fn steam_portrait_candidates(app_id: &str) -> Vec<GameArtCandidate> {
    [
        (STEAM_CAPSULE_2X_PATH, "library_600x900_2x", 1200, 1800),
        (STEAM_CAPSULE_PATH, "library_600x900", 600, 900),
    ]
    .into_iter()
    .filter_map(|(path, variant, width, height)| {
        GameArtCandidate::https(
            format!("{STEAM_CDN_BASE}/{app_id}/{path}"),
            GameArtSource::SteamOfficialCdn,
            variant,
            width,
            height,
        )
    })
    .collect()
}

async fn first_available_with_probe<F, Fut>(
    candidates: &[GameArtCandidate],
    mut probe: F,
) -> Result<Option<RemoteFound>, ()>
where
    F: FnMut(GameArtCandidate) -> Fut,
    Fut: Future<Output = RemoteProbe>,
{
    for candidate in candidates {
        match probe(candidate.clone()).await {
            RemoteProbe::Found(bytes) => {
                let Some((width, height)) = launcher_scan::art::verified_image_bytes(&bytes) else {
                    return Err(());
                };
                return Ok(Some(RemoteFound {
                    candidate: candidate.clone(),
                    bytes,
                    width,
                    height,
                }));
            }
            RemoteProbe::Missing => continue,
            RemoteProbe::Failed => return Err(()),
        }
    }
    Ok(None)
}

async fn http_probe(client: &reqwest::Client, candidate: GameArtCandidate) -> RemoteProbe {
    if !candidate_host_allowed(&candidate) {
        return RemoteProbe::Failed;
    }
    let response = match client.get(&candidate.locator).send().await {
        Ok(response) => response,
        Err(_) => return RemoteProbe::Failed,
    };
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return RemoteProbe::Missing;
    }
    if !response.status().is_success() {
        return RemoteProbe::Failed;
    }
    if response
        .content_length()
        .is_some_and(|length| length > ART_PROTOCOL_MAX_ASSET_BYTES)
    {
        return RemoteProbe::Failed;
    }
    match response.bytes().await {
        Ok(bytes) if bytes.len() as u64 <= ART_PROTOCOL_MAX_ASSET_BYTES => {
            RemoteProbe::Found(bytes.to_vec())
        }
        Ok(_) => RemoteProbe::Failed,
        Err(_) => RemoteProbe::Failed,
    }
}

fn candidate_host_allowed(candidate: &GameArtCandidate) -> bool {
    let Ok(parsed) = url::Url::parse(&candidate.locator) else {
        return false;
    };
    if parsed.scheme() != "https" {
        return false;
    }
    let Some(host) = parsed.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    match candidate.source {
        GameArtSource::SteamOfficialCdn => host.ends_with("steamstatic.com"),
        GameArtSource::SteamGridDb => host.ends_with("steamgriddb.com"),
        GameArtSource::EpicManifest | GameArtSource::EpicCatalog => {
            (host == "epicgames.com" || host.ends_with(".epicgames.com"))
                || host.ends_with(".epicgames.dev")
                || host.ends_with(".unrealengine.com")
                || host.ends_with("akamaized.net")
                || host.ends_with("cloudfront.net")
        }
        _ => false,
    }
}

fn art_cache_root(state: &AppState) -> AppResult<PathBuf> {
    Ok(app_cache_dir(state)?.join("game-art"))
}

fn app_cache_dir(state: &AppState) -> AppResult<PathBuf> {
    state
        .paths
        .read()
        .as_ref()
        .map(|paths| paths.cache_dir.clone())
        .ok_or_else(|| AppError::Other("application paths are not initialized".to_string()))
}

fn cache_stem(key: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(key.as_bytes());
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn cache_metadata_path(root: &Path, key: &str) -> PathBuf {
    root.join(format!("{}.json", cache_stem(key)))
}

fn load_cached_art(root: &Path, key: &str, now: i64) -> Option<GameArt> {
    let raw = std::fs::read(cache_metadata_path(root, key)).ok()?;
    let mut entry: ArtCacheEntry = serde_json::from_slice(&raw).ok()?;
    if entry.expires_at <= now {
        return None;
    }
    if entry.art.state == GameArtState::Resolved {
        for asset in [entry.art.landscape.as_ref(), entry.art.portrait.as_ref()]
            .into_iter()
            .flatten()
        {
            if asset.locator_kind != ArtLocatorKind::LocalFile
                || launcher_scan::art::verified_dimensions(Path::new(&asset.locator)).is_none()
            {
                return None;
            }
        }
    }
    entry.art.cache_status = ArtCacheStatus::Hit;
    Some(entry.art)
}

fn should_persist_art(art: &GameArt) -> bool {
    matches!(
        art.state,
        GameArtState::Resolved | GameArtState::Unavailable
    )
}

fn store_cached_art(root: &Path, key: &str, mut art: GameArt, now: i64) -> AppResult<GameArt> {
    if !should_persist_art(&art) {
        return Ok(art);
    }
    std::fs::create_dir_all(root)?;
    art.cache_status = ArtCacheStatus::Stored;
    let ttl = if art.state == GameArtState::Resolved {
        ART_RESOLVED_CACHE_TTL_SECS
    } else {
        ART_UNAVAILABLE_CACHE_TTL_SECS
    };
    let entry = ArtCacheEntry {
        expires_at: now.saturating_add(ttl),
        art: art.clone(),
    };
    let destination = cache_metadata_path(root, key);
    let temporary = destination.with_extension("json.tmp");
    let encoded = serde_json::to_vec_pretty(&entry)
        .map_err(|error| AppError::Other(format!("game art cache encode: {error}")))?;
    std::fs::write(&temporary, encoded)?;
    std::fs::rename(temporary, destination)?;
    Ok(art)
}

fn persist_remote_asset(root: &Path, key: &str, found: RemoteFound) -> AppResult<GameArtAsset> {
    let _ = key;
    let cache_dir = root
        .parent()
        .ok_or_else(|| AppError::Other("game art cache root has no parent".to_string()))?;
    let destination = crate::art_transport::persist_verified_bytes(cache_dir, &found.bytes)
        .map_err(|error| AppError::Other(format!("persist game art asset: {error}")))?;
    Ok(GameArtAsset::local(
        &destination,
        found.candidate.source,
        found.candidate.variant,
        found.width,
        found.height,
    ))
}

async fn resolve_steam_official(
    client: &reqwest::Client,
    cache_root: &Path,
    cache_key: &str,
    app_id: &str,
) -> GameArt {
    let landscape = first_available_with_probe(&steam_landscape_candidates(app_id), |candidate| {
        http_probe(client, candidate)
    })
    .await;
    let portrait = first_available_with_probe(&steam_portrait_candidates(app_id), |candidate| {
        http_probe(client, candidate)
    })
    .await;

    let source_failed = landscape.is_err() || portrait.is_err();
    let landscape = match landscape {
        Ok(Some(found)) => persist_remote_asset(cache_root, cache_key, found).ok(),
        Ok(None) | Err(()) => None,
    };
    let portrait = match portrait {
        Ok(Some(found)) => persist_remote_asset(cache_root, cache_key, found).ok(),
        Ok(None) | Err(()) => None,
    };
    if landscape.is_some() || portrait.is_some() {
        GameArt::resolved(landscape, portrait)
    } else if source_failed {
        GameArt::source_failed(GameArtSource::SteamOfficialCdn, "request_failed")
    } else {
        GameArt::unavailable(GameArtSource::SteamOfficialCdn, "all_variants_missing")
    }
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn fetch_steam_art(
    state: State<'_, AppState>,
    app_id: String,
    trigger: ArtResolveTrigger,
) -> AppResult<GameArt> {
    let app_id = app_id.trim();
    if app_id.is_empty() || !app_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(GameArt::unavailable(
            GameArtSource::SteamOfficialCdn,
            "invalid_app_id",
        ));
    }
    let cache_root = art_cache_root(&state)?;
    let cache_key = format!("steam:{app_id}");
    let now = chrono::Utc::now().timestamp();
    if let Some(art) = load_cached_art(&cache_root, &cache_key, now) {
        let mut art = art;
        crate::art_transport::prepare_art(&app_cache_dir(&state)?, &mut art)
            .map_err(|error| AppError::Other(format!("prepare cached game art: {error}")))?;
        return Ok(art);
    }
    // Reading a previously verified local asset does not require network consent.
    let channel = state.distribution_policy.read().channel;
    if !remote_art_allowed(channel, trigger) {
        return Ok(policy_pending());
    }

    let mut art = resolve_steam_official(&state.http_art, &cache_root, &cache_key, app_id).await;
    if art.state != GameArtState::SourceFailed {
        art.cache_status = ArtCacheStatus::Miss;
    }
    store_cached_art(&cache_root, &cache_key, art, now)
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn enrich_game_art(
    state: State<'_, AppState>,
    game: DetectedGame,
    api_key: String,
    trigger: ArtResolveTrigger,
) -> AppResult<GameArt> {
    if game.art.state == GameArtState::Resolved {
        let mut art = game.art;
        crate::art_transport::prepare_art(&app_cache_dir(&state)?, &mut art)
            .map_err(|error| AppError::Other(format!("prepare resolved game art: {error}")))?;
        return Ok(art);
    }
    let cache_root = art_cache_root(&state)?;
    let cache_key = format!("game:{}", game.id);
    let now = chrono::Utc::now().timestamp();
    if let Some(art) = load_cached_art(&cache_root, &cache_key, now) {
        let mut art = art;
        crate::art_transport::prepare_art(&app_cache_dir(&state)?, &mut art)
            .map_err(|error| AppError::Other(format!("prepare cached game art: {error}")))?;
        return Ok(art);
    }
    // Reading a previously verified local asset does not require network consent.
    let channel = state.distribution_policy.read().channel;
    if !remote_art_allowed(channel, trigger) {
        return Ok(policy_pending());
    }

    if game.launcher == LauncherKind::Steam {
        if let Some(app_id) = game.app_id.as_deref() {
            let art =
                resolve_steam_official(&state.http_art, &cache_root, &cache_key, app_id).await;
            if art.state == GameArtState::Resolved || api_key.trim().is_empty() {
                return store_cached_art(&cache_root, &cache_key, art, now);
            }
        }
    }

    if game.launcher == LauncherKind::Epic {
        let art = resolve_epic_official(&state.http_art, &cache_root, &cache_key, &game).await;
        if art.state == GameArtState::Resolved || api_key.trim().is_empty() {
            return store_cached_art(&cache_root, &cache_key, art, now);
        }
    }

    if !game.art.candidates.is_empty() {
        match first_available_with_probe(&game.art.candidates, |candidate| {
            http_probe(&state.http_art, candidate)
        })
        .await
        {
            Ok(Some(found)) => {
                let asset = persist_remote_asset(&cache_root, &cache_key, found)?;
                return store_cached_art(
                    &cache_root,
                    &cache_key,
                    GameArt::resolved(Some(asset), None),
                    now,
                );
            }
            Ok(None) if api_key.trim().is_empty() => {
                return store_cached_art(
                    &cache_root,
                    &cache_key,
                    GameArt::unavailable(GameArtSource::EpicManifest, "observed_cover_missing"),
                    now,
                );
            }
            Err(()) => {
                return Ok(GameArt::source_failed(
                    game.art.candidates[0].source,
                    "request_failed",
                ));
            }
            Ok(None) => {}
        }
    }

    if api_key.trim().is_empty() || game.name.trim().is_empty() {
        return Ok(game.art);
    }
    resolve_steamgriddb(
        &state.http_art,
        &cache_root,
        &cache_key,
        &game.name,
        api_key.trim(),
        now,
    )
    .await
}

/// Read artwork only from an exact namespace and unambiguous product identity.
fn epic_catalog_candidates(
    value: &serde_json::Value,
    game: &DetectedGame,
) -> Result<Vec<GameArtCandidate>, &'static str> {
    let namespace = game
        .native_ids
        .get("catalog_namespace")
        .ok_or("missing_namespace")?;
    let item_id = game.native_ids.get("catalog_item_id");
    let elements = value
        .pointer("/data/Catalog/searchStore/elements")
        .and_then(serde_json::Value::as_array)
        .ok_or("invalid_catalog_response")?;
    let scoped: Vec<_> = elements
        .iter()
        .filter(|offer| offer["namespace"].as_str() == Some(namespace.as_str()))
        .collect();
    let exact: Vec<_> = scoped
        .iter()
        .copied()
        .filter(|offer| {
            offer["items"].as_array().is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item_id.is_some_and(|id| item["id"].as_str() == Some(id.as_str())))
            })
        })
        .collect();
    // Store offers can replace their item ID between builds. Accept the published title only
    // when exactly one offer in the same immutable namespace has that exact title.
    let matching: Vec<_> = if exact.is_empty() {
        scoped
            .into_iter()
            .filter(|offer| {
                offer["title"]
                    .as_str()
                    .is_some_and(|title| title.eq_ignore_ascii_case(game.name.trim()))
            })
            .collect()
    } else {
        exact
    };
    if matching.len() != 1 {
        return Err("ambiguous_or_missing_offer");
    }
    let images = matching[0]["keyImages"]
        .as_array()
        .ok_or("missing_images")?;
    let mut candidates = Vec::new();
    for kind in [
        "OfferImageWide",
        "DieselStoreFrontWide",
        "OfferImageTall",
        "DieselStoreFrontTall",
        "Thumbnail",
    ] {
        for image in images
            .iter()
            .filter(|image| image["type"].as_str() == Some(kind))
        {
            if let Some(url) = image["url"].as_str() {
                if let Some(candidate) = GameArtCandidate::https(
                    url,
                    GameArtSource::EpicCatalog,
                    kind,
                    image["width"].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32,
                    image["height"].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32,
                ) {
                    if candidate_host_allowed(&candidate) {
                        candidates.push(candidate);
                    }
                }
            }
        }
    }
    if candidates.is_empty() {
        Err("missing_cover_variants")
    } else {
        Ok(candidates)
    }
}

async fn resolve_epic_official(
    client: &reqwest::Client,
    cache_root: &Path,
    cache_key: &str,
    game: &DetectedGame,
) -> GameArt {
    let Some(namespace) = game.native_ids.get("catalog_namespace") else {
        return GameArt::unavailable(GameArtSource::EpicCatalog, "missing_namespace");
    };
    let query = "query Cover($namespace: String!) { Catalog { searchStore(namespace: $namespace, count: 50, country: \"US\", locale: \"en-US\") { elements { id title namespace keyImages { type url width height } items { id namespace } } } } }";
    let variables = serde_json::json!({"namespace": namespace}).to_string();
    let response = match client
        .get("https://store.epicgames.com/graphql")
        .query(&[("query", query), ("variables", variables.as_str())])
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => response,
        _ => return GameArt::source_failed(GameArtSource::EpicCatalog, "catalog_request_failed"),
    };
    if response
        .content_length()
        .is_some_and(|size| size > 2 * 1024 * 1024)
    {
        return GameArt::source_failed(GameArtSource::EpicCatalog, "catalog_response_too_large");
    }
    let bytes = match response.bytes().await {
        Ok(bytes) if bytes.len() <= 2 * 1024 * 1024 => bytes,
        _ => return GameArt::source_failed(GameArtSource::EpicCatalog, "invalid_catalog_response"),
    };
    let value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => {
            return GameArt::source_failed(GameArtSource::EpicCatalog, "invalid_catalog_response")
        }
    };
    let candidates = match epic_catalog_candidates(&value, game) {
        Ok(candidates) => candidates,
        Err(reason) => return GameArt::unavailable(GameArtSource::EpicCatalog, reason),
    };
    let landscape: Vec<_> = candidates
        .iter()
        .filter(|candidate| candidate.width > candidate.height)
        .cloned()
        .collect();
    let portrait: Vec<_> = candidates
        .iter()
        .filter(|candidate| candidate.height >= candidate.width)
        .cloned()
        .collect();
    let (landscape, portrait) = tokio::join!(
        first_available_with_probe(&landscape, |candidate| http_probe(client, candidate)),
        first_available_with_probe(&portrait, |candidate| http_probe(client, candidate))
    );
    let failed = landscape.is_err() || portrait.is_err();
    let landscape = landscape
        .ok()
        .flatten()
        .and_then(|found| persist_remote_asset(cache_root, cache_key, found).ok());
    let portrait = portrait
        .ok()
        .flatten()
        .and_then(|found| persist_remote_asset(cache_root, cache_key, found).ok());
    if landscape.is_some() || portrait.is_some() {
        GameArt::resolved(landscape, portrait)
    } else if failed {
        GameArt::source_failed(GameArtSource::EpicCatalog, "image_request_failed")
    } else {
        GameArt::unavailable(GameArtSource::EpicCatalog, "cover_variants_missing")
    }
}

async fn resolve_steamgriddb(
    client: &reqwest::Client,
    cache_root: &Path,
    cache_key: &str,
    name: &str,
    api_key: &str,
    now: i64,
) -> AppResult<GameArt> {
    let search_url = format!(
        "{SGDB_API_BASE}/search/autocomplete/{}",
        encode_path(name.trim())
    );
    let response = match client.get(search_url).bearer_auth(api_key).send().await {
        Ok(response) if response.status().is_success() => response,
        Ok(_) | Err(_) => {
            return Ok(GameArt::source_failed(
                GameArtSource::SteamGridDb,
                "search_failed",
            ));
        }
    };
    let search: serde_json::Value = match response.json().await {
        Ok(value) => value,
        Err(_) => {
            return Ok(GameArt::source_failed(
                GameArtSource::SteamGridDb,
                "invalid_response",
            ));
        }
    };
    let Some(game_id) = search
        .get("data")
        .and_then(serde_json::Value::as_array)
        .and_then(|items| items.first())
        .and_then(|game| game.get("id"))
        .and_then(serde_json::Value::as_i64)
    else {
        return store_cached_art(
            cache_root,
            cache_key,
            GameArt::unavailable(GameArtSource::SteamGridDb, "no_match"),
            now,
        );
    };

    let urls = [
        format!("{SGDB_API_BASE}/grids/game/{game_id}?dimensions={SGDB_GRID_DIMS}&types=static"),
        format!("{SGDB_API_BASE}/heroes/game/{game_id}?dimensions={SGDB_HERO_DIMS}&types=static"),
    ];
    let mut candidates = Vec::new();
    for (index, url) in urls.iter().enumerate() {
        let Ok(response) = client.get(url).bearer_auth(api_key).send().await else {
            return Ok(GameArt::source_failed(
                GameArtSource::SteamGridDb,
                "asset_list_failed",
            ));
        };
        if !response.status().is_success() {
            return Ok(GameArt::source_failed(
                GameArtSource::SteamGridDb,
                "asset_list_failed",
            ));
        }
        let Ok(value) = response.json::<serde_json::Value>().await else {
            return Ok(GameArt::source_failed(
                GameArtSource::SteamGridDb,
                "invalid_response",
            ));
        };
        if let Some(items) = value.get("data").and_then(serde_json::Value::as_array) {
            for item in items {
                let Some(url) = item.get("url").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                let width = item
                    .get("width")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as u32;
                let height = item
                    .get("height")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as u32;
                if let Some(candidate) = GameArtCandidate::https(
                    url,
                    GameArtSource::SteamGridDb,
                    if index == 0 { "grid" } else { "hero" },
                    width,
                    height,
                ) {
                    candidates.push(candidate);
                }
            }
        }
    }
    candidates.sort_by(|left, right| {
        u64::from(right.width)
            .saturating_mul(u64::from(right.height))
            .cmp(&u64::from(left.width).saturating_mul(u64::from(left.height)))
    });
    match first_available_with_probe(&candidates, |candidate| http_probe(client, candidate)).await {
        Ok(Some(found)) => {
            let asset = persist_remote_asset(cache_root, cache_key, found)?;
            store_cached_art(
                cache_root,
                cache_key,
                GameArt::resolved(Some(asset), None),
                now,
            )
        }
        Ok(None) => store_cached_art(
            cache_root,
            cache_key,
            GameArt::unavailable(GameArtSource::SteamGridDb, "no_asset"),
            now,
        ),
        Err(()) => Ok(GameArt::source_failed(
            GameArtSource::SteamGridDb,
            "asset_download_failed",
        )),
    }
}

fn encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod art_resolution_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn epic_art_matches_namespace_and_unique_title_not_first_search_hit() {
        let game = DetectedGame {
            id: "epic-app".into(),
            name: "inZOI ModKit".into(),
            launcher: LauncherKind::Epic,
            install_dir: PathBuf::from("C:/Game"),
            app_id: None,
            native_ids: BTreeMap::from([
                ("catalog_namespace".into(), "namespace".into()),
                ("catalog_item_id".into(), "old-item".into()),
            ]),
            art: GameArt::unavailable(GameArtSource::EpicManifest, "missing"),
            image_url: None,
            size_bytes: None,
        };
        let offer = serde_json::json!({"namespace":"namespace","title":"inZOI MODkit","items":[{"id":"new-item"}],"keyImages":[{"type":"OfferImageWide","url":"https://cdn1.epicgames.com/cover.jpg","width":1920,"height":1080},{"type":"OfferImageTall","url":"https://cdn1.epicgames.com/portrait.jpg","width":1200,"height":1600}]});
        let response = serde_json::json!({"data":{"Catalog":{"searchStore":{"elements":[{"namespace":"other","title":"inZOI ModKit"},offer.clone()]}}}});
        let candidates = epic_catalog_candidates(&response, &game).unwrap();
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].source, GameArtSource::EpicCatalog);
        let duplicate = serde_json::json!({"data":{"Catalog":{"searchStore":{"elements":[offer.clone(),offer]}}}});
        assert_eq!(
            epic_catalog_candidates(&duplicate, &game).unwrap_err(),
            "ambiguous_or_missing_offer"
        );
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0_u8; 24];
        bytes[..8].copy_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        bytes
    }

    #[test]
    fn steam_appid_builds_official_candidates_without_name_lookup() {
        let candidates = steam_landscape_candidates("4001890");
        assert_eq!(candidates[0].variant, "library_hero");
        assert_eq!(
            candidates[0].locator,
            "https://cdn.cloudflare.steamstatic.com/steam/apps/4001890/library_hero.jpg"
        );
        assert!(candidates
            .iter()
            .all(|candidate| !candidate.locator.contains("How%20to%20Fish")));
    }

    #[tokio::test]
    async fn steam_variant_fallback_is_ordered_and_missing_is_unavailable() {
        let candidates = steam_portrait_candidates("4001890");
        let mut observed = Vec::new();
        let found = first_available_with_probe(&candidates, |candidate| {
            observed.push(candidate.variant.clone());
            std::future::ready(if candidate.variant == "library_600x900_2x" {
                RemoteProbe::Missing
            } else {
                RemoteProbe::Found(png(600, 900))
            })
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(observed, vec!["library_600x900_2x", "library_600x900"]);
        assert_eq!(found.candidate.variant, "library_600x900");
        assert_eq!((found.width, found.height), (600, 900));

        let missing =
            first_available_with_probe(&candidates, |_| std::future::ready(RemoteProbe::Missing))
                .await
                .unwrap();
        let art = if missing.is_none() {
            GameArt::unavailable(GameArtSource::SteamOfficialCdn, "all_variants_missing")
        } else {
            unreachable!()
        };
        assert_eq!(art.state, GameArtState::Unavailable);
        assert!(!art.retryable);
    }

    #[test]
    fn transient_network_failure_is_retryable_and_not_cached_as_absence() {
        let root = std::env::temp_dir().join("dlssync-art-transient-cache-test");
        let _ = std::fs::remove_dir_all(&root);
        let art = GameArt::source_failed(GameArtSource::SteamOfficialCdn, "request_failed");
        assert_eq!(art.state, GameArtState::SourceFailed);
        assert!(art.retryable);
        assert!(!should_persist_art(&art));
        let returned = store_cached_art(&root, "steam:4001890", art, 1_000).unwrap();
        assert_eq!(returned.cache_status, ArtCacheStatus::TransientFailure);
        assert!(!cache_metadata_path(&root, "steam:4001890").exists());
    }

    #[test]
    fn nexus_automatic_resolution_emits_no_remote_probe() {
        let probes = Arc::new(AtomicUsize::new(0));
        if remote_art_allowed(DistributionChannel::Nexus, ArtResolveTrigger::Automatic) {
            probes.fetch_add(1, Ordering::SeqCst);
        }
        assert_eq!(probes.load(Ordering::SeqCst), 0);
        assert!(remote_art_allowed(
            DistributionChannel::Nexus,
            ArtResolveTrigger::UserScan
        ));
        assert!(remote_art_allowed(
            DistributionChannel::Nexus,
            ArtResolveTrigger::ExplicitRetry
        ));
    }
}
