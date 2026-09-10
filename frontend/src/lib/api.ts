import type { ApplyStage } from "../generated/bindings";
export type { ApplyStage, ApplyProgress_Serialize as ApplyProgress, GroupDownloadProgress, InflightSnapshot } from "../generated/bindings";
import type {
  LauncherKind,
  DetectedGame,
  DllRecord,
  Release,
  BackupEntry,
  DeleteOutcome,
  AppPathsDto,
  SystemInfo,
  DriverRelease as DriverReleaseDto,
  DriverStatusReport,
  CatalogSummary,
  CatalogRefreshTrigger,
  CatalogRefreshResult,
  CatalogStatus as CatalogRuntimeStatus,
  OperationKind,
  OperationStatus,
  OperationRecord,
  GameArt,
  ApplyRequest,
  ApplyResult,
  ApplyBatchRequest,
  ApplyBatchResult,
  StreamlineSetResult,
  AntiCheatReport,
  InstallOutcome as DriverInstallOutcome,
  DriverUpdate as SystemDriverUpdate,
  DeviceGroup as SystemDeviceGroup,
  SystemDriverOutcome,
  DriverInstallContext,
  DriverStoreVersion,
  DlssOverrideConfig,
  OverrideScope,
  DlssOverrideReadback,
  DlssApplyOutcome,
  LogPaths,
  IssueReport
} from "../generated/bindings";
export type {
  LauncherKind,
  DetectedGame,
  DllFamily,
  DllRecord,
  Release,
  BackupEntry,
  DeleteOutcome,
  AppPathsDto,
  GpuVendor,
  OsInfo,
  CpuInfo,
  RamModule,
  RamInfo,
  GpuInfo,
  SystemInfo,
  DriverVersion as DriverVersionDto,
  DriverChangelog as DriverChangelogDto,
  DriverRelease as DriverReleaseDto,
  DeviceId as DeviceIdDto,
  DriverStatusReport,
  CatalogSummary,
  CatalogRefreshTrigger,
  CatalogDelta,
  CatalogProvenance,
  CatalogRefreshResult,
  CatalogStatus as CatalogRuntimeStatus,
  OperationActor,
  OperationKind,
  OperationStatus,
  OperationRecord,
  VendorSummary,
  FamilySummary,
  GameArt,
  ApplyRequest,
  ApplyResult,
  ApplyBatchRequest,
  ApplyOutcome,
  ApplyBatchResult,
  StreamlineSetResult,
  ProtectionKind,
  DetectedAntiCheat,
  AntiCheatReport,
  InstallStage,
  SystemDeviceClass,
  DriverUpdate as SystemDriverUpdate,
  DeviceGroup as SystemDeviceGroup,
  SystemDriverOutcome,
  DriverInstallContext,
  DriverStoreVersion,
  DlssPreset,
  FrameGenMode,
  FrameGenCount,
  DlssOverrideConfig,
  OverrideScope,
  DlssOverrideSource,
  DlssOverrideReadback,
  DlssApplyOutcome,
  LogPaths,
  IssueReport
} from "../generated/bindings";
import { REQUIRED_SETTINGS_PATHS, invokeCommand as transport, COMMANDS } from "../generated/bindings";

export type UpdateStatus =
  | "outdated"
  | "up_to_date"
  | "no_dlls"
  | "unknown"
  | "scanning"
  | "scan_failed";

export type DriverUpdateStatus = "up_to_date" | "update_available" | "unknown" | "unsupported";

export interface JournalFilter {
  target?: string | null;
  kind?: OperationKind | null;
  status?: OperationStatus | null;
  limit?: number | null;
}

export interface LauncherOverrides {
  steam: string[];
  epic: string[];
  gog: string[];
  ubisoft: string[];
  ea_desktop: string[];
  xbox: string[];
  battlenet: string[];
  custom: string[];
}

export interface UpdatePreferences {
  update_dlss: boolean;
  update_dlss_fg: boolean;
  update_dlss_rr: boolean;
  update_streamline: boolean;
  update_reflex: boolean;
  update_xess: boolean;
  update_fsr: boolean;
  update_direct_storage: boolean;
  create_backups: boolean;
  auto_apply_all_on_rescan: boolean;
}

export interface UiPreferences {
  theme: string;
  sidebar_collapsed: boolean;
  grid_density: string;
  sort_order: string;
  launcher_filter: string;
  status_filter: string;
  library_view_mode: LibraryViewMode;
  library_density: LibraryDensity;
  library_sort: LibrarySort;
  backups_group_by: BackupsGroupBy;
  settings_active_tab: SettingsTab;
  command_palette_recent: string[];
  favorite_game_ids: string[];
  show_support_nudge: boolean;
  language: string;
}

export type LibraryViewMode = "grid" | "list";
export type LibraryDensity = "compact" | "comfy";
export type LibrarySort =
  | "default"
  | "outdated_first"
  | "recently_played"
  | "a_z"
  | "z_a"
  | "launcher";
export type BackupsGroupBy = "game" | "date";
export type SettingsTab = "general" | "updates" | "detection" | "art" | "advanced";

export interface SteamApiConfig {
  api_key: string;
  steam_id: string;
}

export interface SgdbConfig {
  api_key: string;
}

export interface WindowState {
  width: number | null;
  height: number | null;
  top: number | null;
  left: number | null;
  maximized: boolean;
}

export interface GamePreference {
  disabled_families: string[];
  pinned_versions: Record<string, string>;
}

export interface AdvancedConfig {
  dlss_debug_overlay: boolean;
  verbose_logs: boolean;
  allow_unsigned_dlls: boolean;
  prefer_stable_channel: boolean;
  apply_concurrency: number;
}

export interface NetworkConfig {
  retry_attempts: number;
  download_cache_ttl_secs: number;
  connect_timeout_secs: number;
  chunk_timeout_secs: number;
}

/** Background-scan daemon settings. Mirrors the Rust `BackgroundConfig`
 *  (commands/settings.rs) field-for-field in snake_case; every field is
 *  `serde(default)` on the Rust side so legacy settings.json migrates cleanly. */
export interface BackgroundConfig {
  enabled: boolean;
  /** Re-scan cadence; the scheduler clamps to 1..=168 when it reads it. */
  interval_hours: number;
  close_to_tray: boolean;
  run_at_startup: boolean;
  notify_os_toast: boolean;
  auto_apply: boolean;
}

export const BACKGROUND_INTERVAL_MIN_HOURS = 1;
export const BACKGROUND_INTERVAL_MAX_HOURS = 168;
export const BACKGROUND_INTERVAL_DEFAULT_HOURS = 24;

export const DEFAULT_BACKGROUND_CONFIG: BackgroundConfig = {
  enabled: false,
  interval_hours: BACKGROUND_INTERVAL_DEFAULT_HOURS,
  close_to_tray: false,
  run_at_startup: false,
  notify_os_toast: true,
  auto_apply: false,
};

export interface AppSettings {
  launcher_overrides: LauncherOverrides;
  update_prefs: UpdatePreferences;
  ui_prefs: UiPreferences;
  steam_api: SteamApiConfig;
  steamgriddb: SgdbConfig;
  window_state: WindowState;
  blacklist: string[];
  ignored: string[];
  game_preferences: Record<string, GamePreference>;
  advanced: AdvancedConfig;
  network: NetworkConfig;
  background: BackgroundConfig;
}

export type { ApplyErrorClass } from "../generated/bindings";

export const APPLY_STAGES: { id: ApplyStage; label: string }[] = [
  { id: "download", label: "Download" },
  { id: "verify_sha", label: "Verify SHA" },
  { id: "verify_signature", label: "Verify signature" },
  { id: "backup", label: "Backup current" },
  { id: "replace", label: "Install new" },
  { id: "verify_post", label: "Verify installed" },
  { id: "complete", label: "Done" },
];

export const APPLY_PROGRESS_EVENT = "apply_progress";
export const DOWNLOAD_PROGRESS_EVENT = "download_progress";
export const APPLY_INFLIGHT_EVENT = "apply_inflight";
export const TRAY_CHECK_UPDATE_EVENT = "tray://check-update";
export const TRAY_SHOW_PROGRESS_EVENT = "tray://show-progress";

/** Backend -> frontend: the background scheduler fired a scan tick. */
export const BACKGROUND_SCAN_TICK_EVENT = "background:scan-tick";
/** Backend -> frontend (tray "Apply all updates"): run the Apply-All flow. */
export const BACKGROUND_APPLY_ALL_EVENT = "background:apply-all";

export const DEFAULT_LAUNCHERS: LauncherKind[] = [
  "steam",
  "epic",
  "gog",
  "ubisoft",
  "ea_desktop",
  "xbox",
  "battlenet",
];

export async function scanLibraries(
  launchers: LauncherKind[] = DEFAULT_LAUNCHERS,
): Promise<DetectedGame[]> {
  return transport(COMMANDS.scan_libraries, { launchers });
}

export async function detectDlls(installDir: string): Promise<DllRecord[]> {
  return transport(COMMANDS.detect_dlls, { installDir });
}

export async function detectDlssEnabler(installDir: string): Promise<boolean> {
  return transport(COMMANDS.detect_dlss_enabler, { installDir });
}

export async function refreshCatalog(
  trigger: CatalogRefreshTrigger,
): Promise<CatalogRefreshResult> {
  return transport(COMMANDS.refresh_catalog, { trigger });
}

export async function getCatalogStatus(): Promise<CatalogRuntimeStatus> {
  return transport(COMMANDS.catalog_status);
}

export async function listJournal(filter: JournalFilter = {}): Promise<OperationRecord[]> {
  return transport(COMMANDS.journal_list, { filter: { target: filter.target ?? null, kind: filter.kind ?? null, status: filter.status ?? null, limit: filter.limit ?? null } });
}

export async function exportJournal(filter: JournalFilter = {}): Promise<string> {
  return transport(COMMANDS.journal_export, { filter: { target: filter.target ?? null, kind: filter.kind ?? null, status: filter.status ?? null, limit: filter.limit ?? null } });
}

export async function catalogSummary(): Promise<CatalogSummary> {
  return transport(COMMANDS.catalog_summary);
}

export async function catalogLatestShas(): Promise<Record<string, string>> {
  return transport(COMMANDS.catalog_latest_shas);
}

export async function listReleases(vendor: string, family: string): Promise<Release[]> {
  return transport(COMMANDS.list_releases, { vendor, family });
}

export async function listBackups(): Promise<BackupEntry[]> {
  return transport(COMMANDS.list_backups);
}

export async function restoreBackup(backupId: string): Promise<void> {
  return transport(COMMANDS.restore_backup, { backupId });
}

export async function deleteBackup(backupId: string): Promise<DeleteOutcome> {
  return transport(COMMANDS.delete_backup, { backupId });
}

export async function getAppPaths(): Promise<AppPathsDto> {
  return transport(COMMANDS.get_app_paths);
}

export async function getSystemInfo(): Promise<SystemInfo> {
  return transport(COMMANDS.get_system_info);
}

export async function checkDriverUpdates(): Promise<DriverStatusReport[]> {
  return transport(COMMANDS.check_driver_updates);
}

export async function listDriverHistory(
  model: string,
  vendor: "nvidia" | "amd" | "intel",
): Promise<DriverReleaseDto[]> {
  return transport(COMMANDS.list_driver_history, { model, vendor });
}

export type ProtectionSource = "binary" | "pe" | "dataset";

export async function detectAnticheat(
  installDir: string,
  appId: string | null,
  name: string,
): Promise<AntiCheatReport> {
  return transport(COMMANDS.detect_anticheat, { installDir, appId, name });
}

export type {
  InstallProgress as DriverInstallProgress,
  InstallOutcome as DriverInstallOutcome,
  SystemDriverInstallProgress as SystemDriverProgress,
  SystemDriverInstallStage as SystemInstallStage,
} from "../generated/bindings";

export const DRIVER_INSTALL_EVENT = "driver_install_progress";

export async function installDriver(
  vendor: string,
  downloadUrl: string,
): Promise<DriverInstallOutcome> {
  return transport(COMMANDS.install_driver, { vendor, downloadUrl });
}

export const SYSTEM_DRIVER_INSTALL_EVENT = "system_driver_install_progress";

/** Installed-device context so the install snapshots the current driver before applying. */

/** One DriverStore version (current or superseded) of a driver package. */

/** Build the snapshot context for a System & Components update from its matched device. */
export function driverInstallContext(
  update: SystemDriverUpdate,
  deviceClass: string,
): DriverInstallContext {
  return {
    infName: update.target_inf ?? null,
    hardwareId: update.target_hardware_id ?? null,
    deviceClass,
    provider: update.provider,
    currentVersion: update.current_version,
  };
}

export async function scanSystemDrivers(): Promise<SystemDeviceGroup[]> {
  return transport(COMMANDS.scan_system_drivers);
}

export async function installSystemDriver(
  updateId: string,
  context?: DriverInstallContext,
): Promise<SystemDriverOutcome> {
  return transport(COMMANDS.install_system_driver, { updateId, context: context ?? null });
}

/** Roll a System & Components driver back to a previously-snapshotted version. */
export async function restoreSystemDriver(backupId: string): Promise<SystemDriverOutcome> {
  return transport(COMMANDS.restore_system_driver, { backupId });
}

/** DriverStore versions (current + superseded) of a driver package, newest-first. */
export async function systemDriverVersions(infName: string): Promise<DriverStoreVersion[]> {
  return transport(COMMANDS.system_driver_versions, { infName });
}

export type { DlssGeneration, NvidiaGpuArchitecture, DlssCapability } from "../generated/bindings";
import type { DlssCapability } from "../generated/bindings";

export async function dlssOverridesSupported(): Promise<boolean> {
  return transport(COMMANDS.dlss_overrides_supported);
}

export async function dlssCapabilities(): Promise<DlssCapability[]> {
  return transport(COMMANDS.dlss_capabilities);
}

export async function applyDlssOverride(
  scope: OverrideScope,
  config: DlssOverrideConfig,
): Promise<DlssApplyOutcome> {
  return transport(COMMANDS.apply_dlss_override, { scope, config });
}

export async function resetDlssOverride(scope: OverrideScope): Promise<void> {
  return transport(COMMANDS.reset_dlss_override, { scope });
}

export async function readDlssOverrideConfig(scope: OverrideScope): Promise<DlssOverrideReadback> {
  return transport(COMMANDS.read_dlss_override_config, { scope });
}

export async function findGameExecutable(installDir: string): Promise<string | null> {
  return transport(COMMANDS.find_game_executable, { installDir });
}

export async function getSettings(): Promise<AppSettings> {
  const value = await transport(COMMANDS.get_settings);
  assertCompleteSettings(value);
  return value;
}

export async function saveSettings(settings: AppSettings): Promise<void> {
  return transport(COMMANDS.save_settings, { settings });
}

export async function addBlacklistEntry(gameId: string): Promise<string[]> {
  return transport(COMMANDS.add_blacklist_entry, { gameId });
}

export async function removeBlacklistEntry(gameId: string): Promise<string[]> {
  return transport(COMMANDS.remove_blacklist_entry, { gameId });
}

export async function addFavoriteGame(gameId: string): Promise<string[]> {
  return transport(COMMANDS.add_favorite_game, { gameId });
}

export async function removeFavoriteGame(gameId: string): Promise<string[]> {
  return transport(COMMANDS.remove_favorite_game, { gameId });
}

export async function saveWindowState(windowState: WindowState): Promise<void> {
  return transport(COMMANDS.save_window_state, { windowState });
}

export async function applyUpdate(request: ApplyRequest): Promise<ApplyResult> {
  return transport(COMMANDS.apply_update, { request });
}

export async function applyUpdateBatch(request: ApplyBatchRequest): Promise<ApplyBatchResult> {
  return transport(COMMANDS.apply_update_batch, { request });
}

export async function applyStreamlineSet(items: ApplyRequest[]): Promise<StreamlineSetResult> {
  return transport(COMMANDS.apply_streamline_set, { items });
}

export async function applyDllSet(items: ApplyRequest[]): Promise<StreamlineSetResult> {
  return transport(COMMANDS.apply_dll_set, { items });
}

export async function cancelApply(applyId: string): Promise<boolean> {
  return transport(COMMANDS.cancel_apply, { applyId });
}

export async function cancelAllApplies(): Promise<number> {
  return transport(COMMANDS.cancel_all_applies);
}

export async function setDlssDebugOverlay(enabled: boolean): Promise<void> {
  return transport(COMMANDS.set_dlss_debug_overlay, { enabled });
}

export async function getDlssDebugOverlay(): Promise<boolean> {
  return transport(COMMANDS.get_dlss_debug_overlay);
}

export async function enrichGameArt(name: string, apiKey: string): Promise<GameArt> {
  return transport(COMMANDS.enrich_game_art, { name, apiKey });
}

export async function fetchSteamArt(name: string): Promise<GameArt> {
  return transport(COMMANDS.fetch_steam_art, { name });
}

export async function openPath(path: string): Promise<void> {
  return transport(COMMANDS.open_path, { path });
}

export async function openUrl(url: string): Promise<void> {
  const { open } = await import("@tauri-apps/plugin-shell");
  await open(url);
}

export async function revealPath(path: string): Promise<void> {
  return transport(COMMANDS.reveal_path, { path });
}

export async function getLogPaths(): Promise<LogPaths> {
  return transport(COMMANDS.get_log_paths);
}

export async function readRecentLogs(maxLines?: number): Promise<string> {
  return transport(COMMANDS.read_recent_logs, { maxLines });
}

export async function buildIssueReport(context?: string): Promise<IssueReport> {
  return transport(COMMANDS.build_issue_report, { context });
}

export async function setEfficiencyMode(enable: boolean): Promise<void> {
  return transport(COMMANDS.set_efficiency_mode, { enable });
}

export async function hideMainWindow(): Promise<void> {
  return transport(COMMANDS.hide_main_window);
}

export async function showMainWindow(): Promise<void> {
  return transport(COMMANDS.show_main_window);
}

/** Set the tray tooltip/badge to the count of games with pending updates.
 *  0 reverts the tray to its idle tooltip. */
export async function traySetPending(count: number): Promise<void> {
  return transport(COMMANDS.tray_set_pending, { count });
}

/** Missing settings are an IPC error, never an instruction to reset preferences. */
function assertCompleteSettings(value: unknown): asserts value is AppSettings {
  for (const path of REQUIRED_SETTINGS_PATHS) {
    let node: unknown = value;
    for (const key of path.split(".")) {
      node = node !== null && typeof node === "object" ? (node as Record<string, unknown>)[key] : undefined;
    }
    if (node === undefined) throw new Error(`Incomplete settings response: ${path}`);
  }
}
