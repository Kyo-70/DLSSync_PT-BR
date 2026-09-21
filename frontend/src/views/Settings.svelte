<script module lang="ts">
  /** The app-update classifier and its wording live in `lib/appUpdateStatus.ts`, shared with the
   *  About surface. That module imports no view, so the two lazily loaded view chunks stay
   *  separate. The names are re-exported here for the suites that read this surface's contract. */
  import {
    classifyUpdateCheck,
    updateStatusText,
    type AppUpdateAvailability,
  } from "../lib/appUpdateStatus";

  export { classifyUpdateCheck, updateStatusText, type AppUpdateAvailability };

  /** What the operating system reports about the startup entry.
   *  `observed` is a read-back value. `unknown` means the entry could not be read, which is not the
   *  same as "off". `mismatch` keeps both values so the difference can be reported instead of being
   *  written away. */
  export type AutostartState =
    | { kind: "unknown"; reason: "not-read" | "unreadable" }
    | { kind: "observed"; enabled: boolean }
    | { kind: "mismatch"; observed: boolean; requested: boolean }
    | { kind: "error"; error: string };

  /** Turn an autostart read-back into state. A non-boolean answer is `unknown`, never `false`.
   *  `requested` is `null` for a plain read with no preceding write. */
  export function classifyAutostartReadback(readback: unknown, requested: boolean | null): AutostartState {
    if (typeof readback !== "boolean") return { kind: "unknown", reason: "unreadable" };
    if (requested === null || readback === requested) return { kind: "observed", enabled: readback };
    return { kind: "mismatch", observed: readback, requested };
  }

  /** Value shown by the startup toggle. It is the read-back value whenever one exists. With no
   *  read-back the stored preference is shown and the row states that it is not verified; the
   *  optimistic value of the pending interaction is never shown. */
  export function autostartCheckboxValue(state: AutostartState, storedPreference: boolean): boolean {
    switch (state.kind) {
      case "observed":
        return state.enabled;
      case "mismatch":
        return state.observed;
      default:
        return storedPreference;
    }
  }
</script>

<script lang="ts">
  import { preferredVendor } from "../lib/hardwarePreference";
  import { hardwarePreference } from "../lib/stores";
  import { onMount, untrack } from "svelte";
  import { fly } from "svelte/transition";
  import PerformanceToggles from "../components/PerformanceToggles.svelte";
  import {
    settings,
    persistSettings, persistUiPreferences,
    loadSettings,
    showToast,
    scanGames,
    currentView,
    driverReports,
    loadDriverUpdates,
  } from "../lib/stores";
  import {
    setDlssDebugOverlay,
    getDlssDebugOverlay,
    revealPath,
    openPath,
    getAppPaths,
  } from "../lib/api";
  import type { AppSettings, UpdatePreferences, LauncherOverrides, AdvancedConfig, NetworkConfig, SgdbConfig, AppPathsDto, BackgroundConfig } from "../lib/api";
  import { BACKGROUND_INTERVAL_MIN_HOURS, BACKGROUND_INTERVAL_MAX_HOURS } from "../lib/api";
  import { LAUNCHER_BRANDS, LAUNCHER_BRAND_ORDER, type LauncherBrandKey } from "../lib/launcherLogos";
  import { resetNudgeSession } from "../lib/community";
  import BrandMark from "../components/BrandMark.svelte";
  import Select from "../components/Select.svelte";
  import { t, locale, loadLocale, translate, LOCALES, LOCALE_LABELS, type Locale } from "../lib/i18n/index";
  import { get } from "svelte/store";
  import type { BrandKey } from "../lib/brands";
  import { appUpdaterEnabled, distributionLabel, isNexusBuild } from "../lib/distribution";
  import { EXTERNAL_URLS } from "../lib/ux";

  let { onToggleTheme, currentTheme }: { onToggleTheme: () => void; currentTheme: string } = $props();
  let dlssOverlayLive = $state(false);
  let appVersion = $state("dev");
  let appPaths = $state<AppPathsDto | null>(null);
  let updateChecking = $state(false);
  let lastUpdateCheck = $state<string | null>(null);
  let updateStatus = $state<AppUpdateAvailability>(appUpdaterEnabled ? { kind: "unchecked" } : { kind: "manualOnly" });
  let autostart = $state<AutostartState>({ kind: "unknown", reason: "not-read" });
  let autostartBusy = $state(false);

  type TabId = "general" | "updates" | "detection" | "art" | "advanced";

  const TAB_IDS: readonly TabId[] = ["general", "updates", "detection", "art", "advanced"];
  function tabFromPref(value: string | undefined | null): TabId {
    return (TAB_IDS as readonly string[]).includes(value ?? "") ? (value as TabId) : "general";
  }

  let activeTab = $state<TabId>(tabFromPref($settings?.ui_prefs.settings_active_tab));

  $effect(() => {
    const persisted = tabFromPref($settings?.ui_prefs.settings_active_tab);
    if (persisted !== untrack(() => activeTab)) activeTab = persisted;
  });

  async function setActiveTab(id: TabId): Promise<void> {
    activeTab = id;
    if (!$settings || $settings.ui_prefs.settings_active_tab === id) return;
    await persistUiPreferences({ settings_active_tab: id });
  }

  async function setShowSupportNudge(on: boolean): Promise<void> {
    if (!$settings) return;
    if (on) resetNudgeSession();
    await persistUiPreferences({ show_support_nudge: on });
  }

  const TABS: { id: TabId; labelKey: string; icon: string }[] = [
    { id: "general", labelKey: "view.settings.tab.general", icon: "settings" },
    { id: "updates", labelKey: "view.settings.tab.updates", icon: "sync" },
    { id: "detection", labelKey: "view.settings.tab.detection", icon: "scan" },
    { id: "art", labelKey: "view.settings.tab.art", icon: "image" },
    { id: "advanced", labelKey: "view.settings.tab.advanced", icon: "flask" },
  ];

  const localeOptions = LOCALES.map((loc) => ({ value: loc, label: LOCALE_LABELS[loc] }));
  let localeChoice = $state<Locale>(get(locale));

  $effect(() => {
    localeChoice = $locale;
  });

  $effect(() => {
    if (localeChoice === get(locale)) return;
    const next = localeChoice;
    void loadLocale(next);
    if ($settings) {
      void persistUiPreferences({ language: next });
    }
  });

  onMount(async () => {
    await loadSettings();
    try {
      dlssOverlayLive = await getDlssDebugOverlay();
    } catch {
      dlssOverlayLive = false;
    }
    try {
      const { getVersion } = await import("@tauri-apps/api/app");
      appVersion = await getVersion();
    } catch {
      appVersion = "dev";
    }
    try {
      appPaths = await getAppPaths();
    } catch {
      appPaths = null;
    }
    await readAutostartState();
    if (!isNexusBuild && $driverReports.length === 0) void loadDriverUpdates();
  });

  async function checkForUpdatesNow(): Promise<void> {
    if (!appUpdaterEnabled) {
      // Nexus policy: no self-updater and no automatic check. The action opens the mod page.
      updateStatus = { kind: "manualOnly" };
      await openReleases();
      return;
    }
    if (updateChecking) return;
    updateChecking = true;
    updateStatus = { kind: "checking" };
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const update = await check();
      lastUpdateCheck = new Date().toLocaleString();
      const status = classifyUpdateCheck(update);
      updateStatus = status;
      if (status.kind === "available") {
        // The install surface is the update banner; it re-reads the release itself.
        window.dispatchEvent(new CustomEvent("dlssync:check-updates", { detail: { force: true } }));
        const v = status.version ?? translate(get(locale), "status.unknown");
        showToast("info", translate(get(locale), "view.settings.updates.toast.available", { version: v }));
      } else if (status.kind === "upToDate") {
        showToast("success", translate(get(locale), "view.settings.updates.toast.latest", { version: appVersion }));
      } else {
        showToast("warning", updateStatusText(get(locale), status, appVersion, distributionLabel));
      }
    } catch (err: unknown) {
      lastUpdateCheck = new Date().toLocaleString();
      updateStatus = { kind: "error", error: String(err) };
      showToast("danger", translate(get(locale), "view.settings.updates.toast.checkFailed", { error: String(err) }));
    } finally {
      updateChecking = false;
    }
  }

  async function openReleases(): Promise<void> {
    const url = appUpdaterEnabled ? EXTERNAL_URLS.releases : EXTERNAL_URLS.nexusMod;
    try {
      const { open } = await import("@tauri-apps/plugin-shell");
      await open(url);
    } catch {
      window.open(url, "_blank");
    }
  }

  async function toggleOverlay(): Promise<void> {
    if (!$settings) return;
    const next = !dlssOverlayLive;
    try {
      await setDlssDebugOverlay(next);
      dlssOverlayLive = next;
      await persistSettings({
        ...$settings,
        advanced: { ...$settings.advanced, dlss_debug_overlay: next },
      });
      showToast("success", translate(get(locale), next ? "view.settings.toast.overlayEnabled" : "view.settings.toast.overlayDisabled"));
    } catch (err: unknown) {
      showToast("danger", translate(get(locale), "view.settings.toast.registryWrite", { error: String(err) }));
    }
  }

  function updateAdvanced<K extends keyof AdvancedConfig>(key: K, value: AdvancedConfig[K]): void {
    if (!$settings) return;
    void persistSettings({
      ...$settings,
      advanced: { ...$settings.advanced, [key]: value },
    });
  }

  function updateNetwork<K extends keyof NetworkConfig>(key: K, value: NetworkConfig[K]): void {
    if (!$settings) return;
    void persistSettings({
      ...$settings,
      network: { ...$settings.network, [key]: value },
    });
  }

  let customFolderInput = $state("");

  function updatePref<K extends keyof UpdatePreferences>(key: K, value: UpdatePreferences[K]): void {
    if (!$settings) return;
    void persistSettings({
      ...$settings,
      update_prefs: { ...$settings.update_prefs, [key]: value },
    });
  }

  function updateBackground<K extends keyof BackgroundConfig>(key: K, value: BackgroundConfig[K]): void {
    if (!$settings) return;
    void persistSettings({
      ...$settings,
      background: { ...$settings.background, [key]: value },
    });
  }

  /** Read the startup entry from the operating system. This is a read; it never registers or
   *  unregisters anything. */
  async function readAutostartState(): Promise<void> {
    try {
      const mod = await import("@tauri-apps/plugin-autostart");
      autostart = classifyAutostartReadback(await mod.isEnabled(), null);
    } catch (err: unknown) {
      autostart = { kind: "error", error: String(err) };
    }
  }

  /** Request a startup-entry change, then show what the operating system reports back.
   *  The requested value is never displayed and never stored on its own: the row shows the
   *  read-back value, a discrepancy is reported to the user, and a failed call leaves the stored
   *  preference untouched instead of writing a compensating value. */
  async function setRunAtStartup(requested: boolean): Promise<void> {
    if (autostartBusy) return;
    autostartBusy = true;
    try {
      const mod = await import("@tauri-apps/plugin-autostart");
      if (requested) {
        await mod.enable();
      } else {
        await mod.disable();
      }
      const state = classifyAutostartReadback(await mod.isEnabled(), requested);
      autostart = state;
      const loc = get(locale);
      if (state.kind === "observed") {
        if ($settings && $settings.background.run_at_startup !== state.enabled) {
          updateBackground("run_at_startup", state.enabled);
        }
      } else if (state.kind === "mismatch") {
        if ($settings && $settings.background.run_at_startup !== state.observed) {
          updateBackground("run_at_startup", state.observed);
        }
        showToast("warning", translate(loc, "view.settings.general.background.runAtStartup.mismatch"));
      } else {
        showToast("warning", translate(loc, "view.settings.general.background.runAtStartup.notVerified"));
      }
    } catch (err: unknown) {
      autostart = { kind: "error", error: String(err) };
      showToast("danger", translate(get(locale), "component.perf.toast.autostartFailed", { error: String(err) }));
    } finally {
      autostartBusy = false;
    }
  }

  const INTERVAL_PRESETS = [1, 6, 12, 24, 48, 72, 168] as const;

  function clampInterval(value: number): number {
    if (!Number.isFinite(value)) return 24;
    return Math.max(BACKGROUND_INTERVAL_MIN_HOURS, Math.min(BACKGROUND_INTERVAL_MAX_HOURS, Math.round(value)));
  }

  function updateOverride<K extends keyof LauncherOverrides>(key: K, value: LauncherOverrides[K]): void {
    if (!$settings) return;
    void persistSettings({
      ...$settings,
      launcher_overrides: { ...$settings.launcher_overrides, [key]: value },
    });
  }

  async function addCustomFolder(): Promise<void> {
    if (!$settings) return;
    const raw = customFolderInput.trim();
    if (!raw) return;
    if ($settings.launcher_overrides.custom.includes(raw)) {
      showToast("warning", translate(get(locale), "view.settings.toast.folderAlreadyAdded"));
      return;
    }
    await persistSettings({
      ...$settings,
      launcher_overrides: {
        ...$settings.launcher_overrides,
        custom: [...$settings.launcher_overrides.custom, raw],
      },
    });
    customFolderInput = "";
    showToast("success", translate(get(locale), "view.settings.toast.folderAdded"));
  }

  async function pickCustomFolder(): Promise<void> {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const result = await open({ directory: true, multiple: false });
      if (typeof result === "string" && result) {
        customFolderInput = result;
        await addCustomFolder();
      }
    } catch (err: unknown) {
      showToast("danger", translate(get(locale), "view.settings.toast.folderPickerFailed", { error: String(err) }));
    }
  }

  async function confirmRemoval(body: string, title: string): Promise<boolean> {
    const loc = get(locale);
    try {
      const { confirm } = await import("@tauri-apps/plugin-dialog");
      return await confirm(body, {
        title,
        kind: "warning",
        okLabel: translate(loc, "view.settings.detection.confirmRemove.ok"),
        cancelLabel: translate(loc, "view.settings.detection.confirmRemove.cancel"),
      });
    } catch {
      return window.confirm(body);
    }
  }

  async function removeCustomFolder(path: string): Promise<void> {
    if (!$settings) return;
    const ok = await confirmRemoval(
      translate(get(locale), "view.settings.detection.confirmRemove.folderBody", { path }),
      translate(get(locale), "view.settings.detection.confirmRemove.folderTitle"),
    );
    if (!ok) return;
    await persistSettings({
      ...$settings,
      launcher_overrides: {
        ...$settings.launcher_overrides,
        custom: $settings.launcher_overrides.custom.filter((p) => p !== path),
      },
    });
  }

  async function removeLauncherPath(launcher: keyof LauncherOverrides, arr: string[], idx: number): Promise<void> {
    const current = arr[idx] ?? "";
    if (current.trim()) {
      const ok = await confirmRemoval(
        translate(get(locale), "view.settings.detection.confirmRemove.pathBody", { path: current }),
        translate(get(locale), "view.settings.detection.confirmRemove.pathTitle"),
      );
      if (!ok) return;
    }
    updateOverride(launcher, arr.filter((_, j) => j !== idx));
  }

  const LAUNCHER_KEYS = LAUNCHER_BRAND_ORDER;
  type LauncherKey = LauncherBrandKey;

  function launcherLabel(key: LauncherKey): string {
    return LAUNCHER_BRANDS[key].label;
  }

  function updateSteamApi<K extends keyof AppSettings["steam_api"]>(
    key: K,
    value: AppSettings["steam_api"][K],
  ): void {
    if (!$settings) return;
    void persistSettings({
      ...$settings,
      steam_api: { ...$settings.steam_api, [key]: value },
    });
  }

  function updateSgdb<K extends keyof SgdbConfig>(key: K, value: SgdbConfig[K]): void {
    if (!$settings) return;
    void persistSettings({
      ...$settings,
      steamgriddb: { ...$settings.steamgriddb, [key]: value },
    });
  }

  async function openSgdbPrefs(): Promise<void> {
    try {
      const { open } = await import("@tauri-apps/plugin-shell");
      await open("https://www.steamgriddb.com/profile/preferences/api");
    } catch {
      window.open("https://www.steamgriddb.com/profile/preferences/api", "_blank");
    }
  }

  async function openSteamKey(): Promise<void> {
    try {
      const { open } = await import("@tauri-apps/plugin-shell");
      await open("https://steamcommunity.com/dev/apikey");
    } catch {
      window.open("https://steamcommunity.com/dev/apikey", "_blank");
    }
  }

  async function revealConfigFile(): Promise<void> {
    if (!appPaths) {
      showToast("warning", translate(get(locale), "view.settings.toast.pathsUnavailable"));
      return;
    }
    try {
      await revealPath(appPaths.settings_file);
    } catch {
      try {
        await openPath(appPaths.settings_dir);
      } catch (err2: unknown) {
        showToast("danger", translate(get(locale), "view.settings.toast.revealFailed", { error: String(err2) }));
      }
    }
  }

  async function openConfigDir(): Promise<void> {
    if (!appPaths) return;
    try {
      await openPath(appPaths.root);
    } catch (err: unknown) {
      showToast("danger", translate(get(locale), "view.settings.toast.openFailed", { error: String(err) }));
    }
  }

  async function openBackupsDir(): Promise<void> {
    if (!appPaths) return;
    try {
      await openPath(appPaths.backups_dir);
    } catch (err: unknown) {
      showToast("danger", translate(get(locale), "view.settings.toast.openFailed", { error: String(err) }));
    }
  }

  async function openLogsDir(): Promise<void> {
    if (!appPaths) return;
    try {
      await openPath(appPaths.logs_dir);
    } catch (err: unknown) {
      try {
        await openPath(appPaths.root);
      } catch {
        showToast("danger", translate(get(locale), "view.settings.toast.openFailed", { error: String(err) }));
      }
    }
  }

  type FeatureToggle = { key: keyof UpdatePreferences; labelKey: string; subKey: string; files: string | null };
  const featureToggles: FeatureToggle[] = [
    { key: "update_dlss", labelKey: "view.settings.feature.update_dlss.label", subKey: "view.settings.feature.update_dlss.sub", files: "nvngx_dlss.dll · sl.dlss.dll" },
    { key: "update_dlss_fg", labelKey: "view.settings.feature.update_dlss_fg.label", subKey: "view.settings.feature.update_dlss_fg.sub", files: "nvngx_dlssg.dll · sl.dlss_g.dll" },
    { key: "update_dlss_rr", labelKey: "view.settings.feature.update_dlss_rr.label", subKey: "view.settings.feature.update_dlss_rr.sub", files: "nvngx_dlssd.dll · sl.dlss_d.dll" },
    { key: "update_streamline", labelKey: "view.settings.feature.update_streamline.label", subKey: "view.settings.feature.update_streamline.sub", files: "sl.interposer.dll · sl.common.dll · sl.dlss.dll · sl.dlss_g.dll · sl.reflex.dll · sl.pcl.dll · sl.directsr.dll" },
    { key: "update_reflex", labelKey: "view.settings.feature.update_reflex.label", subKey: "view.settings.feature.update_reflex.sub", files: "sl.reflex.dll" },
    { key: "update_xess", labelKey: "view.settings.feature.update_xess.label", subKey: "view.settings.feature.update_xess.sub", files: "libxess.dll · libxess_fg.dll · libxell.dll" },
    { key: "update_fsr", labelKey: "view.settings.feature.update_fsr.label", subKey: "view.settings.feature.update_fsr.sub", files: "amd_fidelityfx_*.dll · ffx_*.dll" },
    { key: "update_direct_storage", labelKey: "view.settings.feature.update_direct_storage.label", subKey: "view.settings.feature.update_direct_storage.sub", files: "dstorage.dll · dstoragecore.dll" },
  ];

  type FeatureGroup = { brand: BrandKey; labelKey: string; keys: (keyof UpdatePreferences)[] };
  const featureGroups: FeatureGroup[] = [
    { brand: "nvidia", labelKey: "view.settings.featureGroup.nvidia", keys: ["update_dlss", "update_dlss_fg", "update_dlss_rr", "update_streamline", "update_reflex"] },
    { brand: "intel", labelKey: "view.settings.featureGroup.intel", keys: ["update_xess"] },
    { brand: "amd", labelKey: "view.settings.featureGroup.amd", keys: ["update_fsr"] },
    { brand: "microsoft", labelKey: "view.settings.featureGroup.microsoft", keys: ["update_direct_storage"] },
  ];
  const featureByKey = new Map(featureToggles.map((ft) => [ft.key, ft]));

  let showFilesFor = $state<Record<string, boolean>>({});
  function toggleFiles(key: string): void {
    showFilesFor = { ...showFilesFor, [key]: !showFilesFor[key] };
  }

  let sgdbKeyMasked = $derived.by(() => {
    const k = $settings?.steamgriddb.api_key ?? "";
    if (!k) return "";
    return k.length > 8 ? `${k.slice(0, 4)}…${k.slice(-4)}` : "•••";
  });

  let enabledFeatureCount = $derived.by(() => {
    if (!$settings) return 0;
    return featureToggles.filter((ft) => $settings!.update_prefs[ft.key]).length;
  });
</script>

<header class="view-header settings-heading">
  <div>
    <h1 class="view-title">{$t("view.settings.title")}</h1>
    <p class="view-subtitle">{$t("view.settings.subtitle")}</p>
  </div>
  {#if $settings}
    <details class="settings-files">
      <summary>{$t("view.settings.filesAndLogs")}<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg></summary>
    <div class="hero-actions" title={appPaths?.settings_file}>
      <button class="btn btn-sm btn-ghost" onclick={revealConfigFile} disabled={!appPaths} title={$t("view.settings.hero.revealConfigTitle")}>
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/></svg>
        {$t("view.settings.hero.revealConfig")}
      </button>
      <button class="btn btn-sm btn-ghost" onclick={openConfigDir} disabled={!appPaths} title={$t("view.settings.hero.dataFolderTitle")}>
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
        {$t("view.settings.hero.dataFolder")}
      </button>
      <button class="btn btn-sm btn-ghost" onclick={openBackupsDir} disabled={!appPaths} title={$t("view.settings.hero.backupsTitle")}>
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="21 8 21 21 3 21 3 8"/><rect x="1" y="3" width="22" height="5" rx="0.5"/><line x1="10" y1="12" x2="14" y2="12"/></svg>
        {$t("view.settings.hero.backups")}
      </button>
      <button class="btn btn-sm btn-ghost" onclick={openLogsDir} disabled={!appPaths} title={$t("view.settings.hero.logsTitle")}>
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="13 2 13 9 20 9"/><path d="M13 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><line x1="9" y1="13" x2="15" y2="13"/><line x1="9" y1="17" x2="15" y2="17"/></svg>
        {$t("view.settings.hero.logs")}
      </button>
    </div>
      <p class="settings-file-meta">v{appVersion} · {$t("view.settings.hero.technologiesEnabled", { count: enabledFeatureCount, total: featureToggles.length })}</p>
    </details>
  {/if}
</header>

{#if !$settings}
  <div class="loading">{$t("view.settings.loading")}</div>
{:else}
  <label class="settings-section-picker">
    <span>{$t("view.settings.sectionsAria")}</span>
    <select value={activeTab} onchange={(event) => void setActiveTab((event.target as HTMLSelectElement).value as TabId)}>
      {#each TABS as tab}<option value={tab.id}>{$t(tab.labelKey)}</option>{/each}
    </select>
  </label>
  <div class="settings-layout">
    <nav class="side-nav" aria-label={$t("view.settings.sectionsAria")}>
      {#each TABS as tab}
        <button class="side-tab" aria-pressed={activeTab === tab.id} class:active={activeTab === tab.id} onclick={() => void setActiveTab(tab.id)}>
          <span class="side-tab-icon" aria-hidden="true">
            {#if tab.icon === "settings"}
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
            {:else if tab.icon === "sync"}
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/></svg>
            {:else if tab.icon === "scan"}
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7V4a1 1 0 0 1 1-1h3M17 3h3a1 1 0 0 1 1 1v3M21 17v3a1 1 0 0 1-1 1h-3M7 21H4a1 1 0 0 1-1-1v-3"/><circle cx="12" cy="12" r="3"/></svg>
            {:else if tab.icon === "image"}
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>
            {:else if tab.icon === "flask"}
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 2v6L4 20a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2L15 8V2"/><line x1="8" y1="2" x2="16" y2="2"/></svg>
            {/if}
          </span>
          <span class="side-tab-label">{$t(tab.labelKey)}</span>
          <span class="side-tab-rail" aria-hidden="true"></span>
        </button>
      {/each}
    </nav>

  <div class="tab-panels">
    {#if activeTab === "general"}
      <section in:fly={{ y: 4, duration: 200 }}>
        <header class="section-head">
          <h2 class="section-title-h">{$t("view.settings.general.appearance.title")}</h2>
          <p class="section-help">{$t("view.settings.general.appearance.help")}</p>
        </header>
        <div class="card">
          <div class="row">
            <div class="row-text">
              <div class="row-label">{$t("view.settings.general.darkTheme.label")}</div>
              <div class="row-sub">{$t("view.settings.general.darkTheme.sub")}</div>
            </div>
            <label class="toggle">
              <input type="checkbox" checked={currentTheme === "dark"} aria-label={$t("view.settings.general.darkTheme.label")} onchange={onToggleTheme} />
              <span class="toggle-slider"></span>
            </label>
          </div>
          <div class="row row-divider">
            <div class="row-text">
              <div class="row-label">{$t("language.label")}</div>
              <div class="row-sub">{$t("view.settings.general.language.sub")}</div>
            </div>
            <div class="lang-select">
              <Select
                bind:value={localeChoice}
                options={localeOptions}
                ariaLabel={$t("language.switcherAria")}
              />
            </div>
          </div>
        </div>

        <header class="section-head section-head-gap">
          <h2 class="section-title-h">{$t("view.settings.general.performance.title")}</h2>
          <p class="section-help">{$t("view.settings.general.performance.help")}</p>
        </header>
        <PerformanceToggles />

        <header class="section-head section-head-gap">
          <h2 class="section-title-h">
            {$t("view.settings.general.background.title")}
            <span class="chip chip-update small-pill">{$t("view.settings.general.background.badge")}</span>
          </h2>
          <p class="section-help">{$t("view.settings.general.background.help")}</p>
        </header>
        <div class="card">
          <div class="row">
            <div class="row-text">
              <div class="row-label">{$t("view.settings.general.background.enabled.label")}</div>
              <div class="row-sub">{$t("view.settings.general.background.enabled.sub")}</div>
            </div>
            <label class="toggle">
              <input
                type="checkbox"
                checked={$settings.background.enabled}
                aria-label={$t("view.settings.general.background.enabled.label")}
                onchange={(e) => updateBackground("enabled", (e.target as HTMLInputElement).checked)}
              />
              <span class="toggle-slider"></span>
            </label>
          </div>

          <div class="row row-divider" class:row-disabled={!$settings.background.enabled}>
            <div class="row-text">
              <label class="row-label" for="bg-interval">{$t("view.settings.general.background.interval.label")}</label>
              <div class="row-sub">{$t("view.settings.general.background.interval.sub")}</div>
            </div>
            <select
              id="bg-interval"
              class="sort-select interval-select"
              value={String(clampInterval($settings.background.interval_hours))}
              disabled={!$settings.background.enabled}
              onchange={(e) => updateBackground("interval_hours", clampInterval(Number((e.currentTarget as HTMLSelectElement).value)))}
            >
              {#each INTERVAL_PRESETS as hrs (hrs)}
                <option value={String(hrs)}>{$t("view.settings.general.background.interval.option", { count: hrs })}</option>
              {/each}
            </select>
          </div>

          <div class="row row-divider">
            <div class="row-text">
              <div class="row-label">{$t("view.settings.general.background.closeToTray.label")}</div>
              <div class="row-sub">{$t("view.settings.general.background.closeToTray.sub")}</div>
            </div>
            <label class="toggle">
              <input
                type="checkbox"
                checked={$settings.background.close_to_tray}
                aria-label={$t("view.settings.general.background.closeToTray.label")}
                onchange={(e) => updateBackground("close_to_tray", (e.target as HTMLInputElement).checked)}
              />
              <span class="toggle-slider"></span>
            </label>
          </div>

          <div class="row row-divider">
            <div class="row-text" data-testid="autostart-state" data-state={autostart.kind}>
              <div class="row-label">{$t("view.settings.general.background.runAtStartup.label")}</div>
              <div class="row-sub">{$t("view.settings.general.background.runAtStartup.sub")}</div>
              {#if autostart.kind === "unknown"}
                <div class="row-sub row-sub-caution">
                  {translate($locale, "view.settings.general.background.runAtStartup.notVerified")}
                </div>
              {:else if autostart.kind === "mismatch"}
                <div class="row-sub row-sub-caution">
                  {translate($locale, "view.settings.general.background.runAtStartup.mismatch")}
                </div>
              {:else if autostart.kind === "error"}
                <div class="row-sub row-sub-caution">
                  <!-- `error` is the raw failure text from the operating system, an open wire
                       string: inserted as a value, never translated. -->
                  {translate($locale, "view.settings.general.background.runAtStartup.readFailed", { error: autostart.error })}
                </div>
              {/if}
            </div>
            <label class="toggle">
              <input
                type="checkbox"
                checked={autostartCheckboxValue(autostart, $settings.background.run_at_startup)}
                aria-label={$t("view.settings.general.background.runAtStartup.label")}
                disabled={autostartBusy}
                onchange={(e) => {
                  const input = e.currentTarget as HTMLInputElement;
                  const requested = input.checked;
                  // The control keeps showing the last value read back from the system. The
                  // requested value is never displayed; the new read-back replaces it.
                  input.checked = autostartCheckboxValue(autostart, $settings.background.run_at_startup);
                  void setRunAtStartup(requested);
                }}
              />
              <span class="toggle-slider"></span>
            </label>
          </div>

          <div class="row row-divider" class:row-disabled={!$settings.background.enabled}>
            <div class="row-text">
              <div class="row-label">{$t("view.settings.general.background.notifyToast.label")}</div>
              <div class="row-sub">{$t("view.settings.general.background.notifyToast.sub")}</div>
            </div>
            <label class="toggle">
              <input
                type="checkbox"
                checked={$settings.background.notify_os_toast}
                aria-label={$t("view.settings.general.background.notifyToast.label")}
                disabled={!$settings.background.enabled}
                onchange={(e) => updateBackground("notify_os_toast", (e.target as HTMLInputElement).checked)}
              />
              <span class="toggle-slider"></span>
            </label>
          </div>

          <div class="row row-divider" class:row-disabled={!$settings.background.enabled}>
            <div class="row-text">
              <div class="row-label">
                {$t("view.settings.general.background.autoApply.label")}
                <span class="chip chip-warning small-pill">{$t("view.settings.general.background.autoApply.badge")}</span>
              </div>
              <div class="row-sub">{$t("view.settings.general.background.autoApply.sub")}</div>
            </div>
            <label class="toggle">
              <input
                type="checkbox"
                checked={$settings.background.auto_apply}
                aria-label={$t("view.settings.general.background.autoApply.label")}
                disabled={!$settings.background.enabled}
                onchange={(e) => updateBackground("auto_apply", (e.target as HTMLInputElement).checked)}
              />
              <span class="toggle-slider"></span>
            </label>
          </div>
        </div>

        <header class="section-head section-head-gap">
          <h2 class="section-title-h">{$t("view.settings.general.support.title")}</h2>
          <p class="section-help">{$t("view.settings.general.support.help")}</p>
        </header>
        <div class="card">
          <div class="row">
            <div class="row-text">
              <div class="row-label">{$t("view.settings.general.support.label")}</div>
              <div class="row-sub">{$t("view.settings.general.support.sub")}</div>
            </div>
            <label class="toggle">
              <input
                type="checkbox"
                checked={$settings.ui_prefs.show_support_nudge}
                aria-label={$t("view.settings.general.support.label")}
                onchange={(e) => void setShowSupportNudge((e.target as HTMLInputElement).checked)}
              />
              <span class="toggle-slider"></span>
            </label>
          </div>
        </div>
      </section>

    {:else if activeTab === "updates"}
      <section in:fly={{ y: 4, duration: 200 }}>
        <header class="section-head">
          <h2 class="section-title-h">{appUpdaterEnabled ? $t("view.settings.updates.autoUpdate.title") : $t("view.settings.updates.autoUpdate.titleManual")}</h2>
          <p class="section-help">{appUpdaterEnabled ? $t("view.settings.updates.autoUpdate.help") : $t("view.settings.updates.autoUpdate.helpManual", { distribution: distributionLabel })}</p>
        </header>
        <div class="card">
          <div class="row">
            <div class="row-text">
              <div class="row-label">{$t("view.settings.updates.currentVersion.label")}</div>
              <div class="row-sub mono">
                v{appVersion}{#if appUpdaterEnabled && lastUpdateCheck}  ·  {$t("view.settings.updates.lastCheck", { time: lastUpdateCheck })}{/if}
              </div>
              <div
                class="row-sub update-status"
                class:update-status-caution={updateStatus.kind === "indeterminate" || updateStatus.kind === "error"}
                data-testid="app-update-status"
                data-status={updateStatus.kind}
              >
                {updateStatusText($locale, updateStatus, appVersion, distributionLabel)}
              </div>
            </div>
            <button class="btn btn-primary" onclick={checkForUpdatesNow} disabled={appUpdaterEnabled && updateChecking}>
              {#if appUpdaterEnabled}
                {updateChecking ? $t("view.settings.updates.checking") : $t("view.settings.updates.checkNow")}
              {:else}
                {$t("view.settings.updates.openReleases")}
              {/if}
            </button>
          </div>
        </div>

        <header class="section-head section-head-gap">
          <h2 class="section-title-h">{$t("view.settings.updates.prefs.title")}</h2>
          <p class="section-help">{$t("view.settings.updates.prefs.help")}</p>
        </header>
        {#each featureGroups as group (group.brand)}
          <details class="feature-group" open={preferredVendor(group.brand, $hardwarePreference)}>
            <summary class="feature-group-head">
              <span class="feature-group-logo" aria-hidden="true"><BrandMark key={group.brand} tone="color" size={20} fit="wordmark" showLabel={false} /></span>
              <span class="feature-group-label">{$t(group.labelKey)}</span>
            </summary>
            <div class="card">
              {#each group.keys as fkey, i (fkey)}
                {@const ft = featureByKey.get(fkey)}
                {#if ft}
                  <div class="row" class:row-divider={i > 0}>
                    <div class="row-text">
                      <div class="row-label">{$t(ft.labelKey)}</div>
                      <div class="row-sub">{$t(ft.subKey)}</div>
                      {#if ft.files}
                        <button class="files-disclosure" onclick={() => toggleFiles(ft.key)} aria-expanded={!!showFilesFor[ft.key]}>
                          <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" class="chev" class:open={!!showFilesFor[ft.key]}><polyline points="6 9 12 15 18 9"/></svg>
                          {showFilesFor[ft.key] ? $t("view.settings.filesDisclosure.hide") : $t("view.settings.filesDisclosure.show")}
                        </button>
                        {#if showFilesFor[ft.key]}
                          <div class="files-meta mono">{ft.files}</div>
                        {/if}
                      {/if}
                    </div>
                    <label class="toggle">
                      <input
                        type="checkbox"
                        checked={$settings.update_prefs[ft.key]}
                        aria-label={$t(ft.labelKey)}
                        onchange={(e) => updatePref(ft.key, (e.target as HTMLInputElement).checked)}
                      />
                      <span class="toggle-slider"></span>
                    </label>
                  </div>
                {/if}
              {/each}
            </div>
          </details>
        {/each}
      </section>

    {:else if activeTab === "detection"}
      {#snippet launcherLogo(key: LauncherKey)}
        <span class="launcher-logo" style:background={LAUNCHER_BRANDS[key].bg} aria-hidden="true">
          <svg viewBox="0 0 24 24" width="22" height="22" fill="#ffffff" xmlns="http://www.w3.org/2000/svg">
            <path d={LAUNCHER_BRANDS[key].path} />
          </svg>
        </span>
      {/snippet}

      <section in:fly={{ y: 4, duration: 200 }}>
        <header class="section-head">
          <h2 class="section-title-h" id="detection-custom-folders-title">{$t("view.settings.detection.customFolders.title")}</h2>
          <p class="section-help">{$t("view.settings.detection.customFolders.help", { path: "C:\\Games" })}</p>
        </header>
        <div class="card">
          <div class="folder-input-row">
            <div class="folder-input-wrap">
              <svg class="folder-input-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
              <input
                type="text"
                aria-labelledby="detection-custom-folders-title"
                placeholder="C:\Games"
                bind:value={customFolderInput}
                onkeydown={(e) => { if (e.key === "Enter") void addCustomFolder(); }}
              />
            </div>
            <button class="aura-pill aura-pill-ghost" onclick={pickCustomFolder}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
              {$t("view.settings.detection.browse")}
            </button>
            <button class="aura-pill aura-pill-primary" onclick={addCustomFolder} disabled={!customFolderInput.trim()}>
              {$t("view.settings.detection.addFolder")}
            </button>
          </div>
          {#if $settings.launcher_overrides.custom.length === 0}
            <div class="empty-state">
              <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" opacity="0.4"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
              <span>{$t("view.settings.detection.noFolders")}</span>
            </div>
          {:else}
            <ul class="path-list">
              {#each $settings.launcher_overrides.custom as p (p)}
                <li class="path-row">
                  <span class="path-icon aura-badge" data-tint="blue">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
                  </span>
                  <span class="path-text mono">{p}</span>
                  <button class="path-action" title={$t("view.settings.detection.openFolderTitle")} onclick={() => openPath(p).catch((err) => showToast("danger", translate(get(locale), "view.settings.toast.openFailed", { error: String(err) })))}>
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
                  </button>
                  <button class="path-action path-action-danger" title={$t("view.settings.detection.removeTitle")} onclick={() => removeCustomFolder(p)}>
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6l-2 14a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2L5 6"/><path d="M10 11v6M14 11v6"/></svg>
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
          <div class="row-actions">
            <!-- An explicit user action. `user_scan` is the consent trigger the art resolver needs;
                 the default `automatic` trigger belongs to background and startup scans. -->
            <button class="aura-pill aura-pill-ghost" onclick={() => scanGames({ trigger: "user_scan" })}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="23 4 23 10 17 10"/><path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/></svg>
              {$t("view.settings.detection.rescanNow")}
            </button>
          </div>
        </div>

        <header class="section-head section-head-gap">
          <h2 class="section-title-h">{$t("view.settings.detection.launcherOverrides.title")}</h2>
          <p class="section-help">{$t("view.settings.detection.launcherOverrides.help")}</p>
        </header>
        <div class="card launcher-card">
          {#each LAUNCHER_KEYS as launcher, i}
            {@const arr = ($settings.launcher_overrides as unknown as Record<string, string[]>)[launcher] ?? []}
            <div class="launcher-row" class:row-divider={i > 0}>
              <div class="launcher-head">
                {@render launcherLogo(launcher)}
                <div class="launcher-head-text">
                  <div class="row-label" id={`launcher-${launcher}-label`}>{launcherLabel(launcher)}</div>
                  <div class="row-sub">{arr.length === 0 ? $t("view.settings.detection.launcher.defaultAuto") : $t("view.settings.detection.launcher.overrideCount", { count: arr.length })}</div>
                </div>
              </div>
              <div class="launcher-input-col">
                {#each arr as p, idx (idx)}
                  <div class="path-input-row">
                    <input
                      type="text"
                      aria-labelledby={`launcher-${launcher}-label`}
                      value={p}
                      placeholder="C:\Program Files\..."
                      onchange={(e) => {
                        const next = [...arr];
                        next[idx] = (e.target as HTMLInputElement).value;
                        updateOverride(launcher as keyof LauncherOverrides, next);
                      }}
                    />
                    <button class="path-remove" title={$t("view.settings.detection.launcher.removePathTitle")} onclick={() => void removeLauncherPath(launcher as keyof LauncherOverrides, arr, idx)}>
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><line x1="6" y1="6" x2="18" y2="18"/><line x1="6" y1="18" x2="18" y2="6"/></svg>
                    </button>
                  </div>
                {/each}
                <button class="add-path-pill" onclick={() => updateOverride(launcher as keyof LauncherOverrides, [...arr, ""])}>
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
                  {$t("view.settings.detection.launcher.addPath")}
                </button>
              </div>
            </div>
          {/each}
        </div>
      </section>

    {:else if activeTab === "art"}
      <section in:fly={{ y: 4, duration: 200 }}>
        <header class="section-head">
          <h2 class="section-title-h">{$t("view.settings.art.steamCdn.title")}</h2>
          <p class="section-help">{$t("view.settings.art.steamCdn.help")}</p>
        </header>
        <div class="card">
          <div class="row">
            <div class="row-text">
              <div class="row-label">{$t("view.settings.art.steamCdn.alwaysOn")} <span class="chip chip-success small-pill">{$t("view.settings.art.steamCdn.noKeyRequired")}</span></div>
              <div class="row-sub">{$t("view.settings.art.steamCdn.sub")}</div>
            </div>
          </div>
        </div>

        <header class="section-head section-head-gap">
          <h2 class="section-title-h">SteamGridDB <span class="section-tag">{$t("view.settings.art.sgdb.fallback")}</span></h2>
          <p class="section-help">{$t("view.settings.art.sgdb.help")}</p>
          <p class="section-help muted">{$t("view.settings.art.sgdb.helpMuted")}</p>
        </header>
        <div class="card">
          <div class="row art-row">
            <div class="row-text">
              <div class="row-label" id="art-sgdb-key-label">{$t("view.settings.art.sgdb.label")} {#if sgdbKeyMasked}<span class="chip chip-update small-pill">{$t("view.settings.art.sgdb.active", { masked: sgdbKeyMasked })}</span>{/if}</div>
              <div class="row-sub" id="art-sgdb-key-sub">{$t("view.settings.art.sgdb.sub", { file: "settings.json" })}</div>
              <button class="files-disclosure" onclick={openSgdbPrefs}>
                <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/></svg>
                {$t("view.settings.art.sgdb.getKey")}
              </button>
            </div>
            <input
              type="password"
              aria-labelledby="art-sgdb-key-label"
              aria-describedby="art-sgdb-key-sub"
              placeholder={$t("view.settings.art.sgdb.placeholder")}
              value={$settings.steamgriddb.api_key}
              onchange={(e) => updateSgdb("api_key", (e.target as HTMLInputElement).value)}
            />
          </div>
        </div>

        <header class="section-head section-head-gap">
          <h2 class="section-title-h">Steam Web API <span class="section-tag">{$t("view.settings.art.steamApi.optional")}</span></h2>
          <p class="section-help">{$t("view.settings.art.steamApi.help", { manifest: "appmanifest_*.acf", site: "steamcommunity.com/dev/apikey" })}</p>
          <p class="section-help muted">{$t("view.settings.art.steamApi.helpMuted", { file: "settings.json" })}</p>
        </header>
        <div class="card">
          <div class="row art-row">
            <div class="row-text">
              <div class="row-label" id="art-steam-key-label">{$t("view.settings.art.steamApi.keyLabel")}</div>
              <div class="row-sub" id="art-steam-key-sub">{$t("view.settings.art.steamApi.keySub")}</div>
              <button class="files-disclosure" onclick={openSteamKey}>
                <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/></svg>
                steamcommunity.com/dev/apikey
              </button>
            </div>
            <input
              type="password"
              aria-labelledby="art-steam-key-label"
              aria-describedby="art-steam-key-sub"
              placeholder={$t("view.settings.art.steamApi.keyPlaceholder")}
              value={$settings.steam_api.api_key}
              onchange={(e) => updateSteamApi("api_key", (e.target as HTMLInputElement).value)}
            />
          </div>
          <div class="row art-row row-divider">
            <div class="row-text">
              <div class="row-label" id="art-steam-id-label">{$t("view.settings.art.steamApi.idLabel")}</div>
              <div class="row-sub" id="art-steam-id-sub">{$t("view.settings.art.steamApi.idSub", { file: "loginusers.vdf" })}</div>
            </div>
            <input
              type="text"
              aria-labelledby="art-steam-id-label"
              aria-describedby="art-steam-id-sub"
              placeholder="76561198xxxxxxxxx"
              value={$settings.steam_api.steam_id}
              onchange={(e) => updateSteamApi("steam_id", (e.target as HTMLInputElement).value)}
            />
          </div>
        </div>
      </section>

    {:else if activeTab === "advanced"}
      <section in:fly={{ y: 4, duration: 200 }} class="tab-section">
        <header class="section-head">
          <h2 class="section-title-h">{$t("view.settings.advanced.optionsTitle")}</h2>
          <p class="section-help">{$t("view.settings.advanced.optionsIntro")}</p>
        </header>
        <div class="card">
          <div class="row">
            <div class="row-text">
              <div class="row-label">{$t("view.settings.advanced.overlay.label")}</div>
              <div class="row-sub">{$t("view.settings.advanced.overlay.sub")}</div>
              <details class="setting-detail"><summary>{$t("view.settings.advanced.registryDetails")}</summary><p>{$t("view.settings.advanced.powerUser.help", { regPath: "HKCU\\SOFTWARE\\NVIDIA Corporation\\Global\\NGXCore" })}</p></details>
            </div>
            <label class="toggle">
              <input type="checkbox" checked={dlssOverlayLive} aria-label={$t("view.settings.advanced.overlay.label")} onchange={toggleOverlay} />
              <span class="toggle-slider"></span>
            </label>
          </div>
          <div class="row row-divider">
            <div class="row-text">
              <div class="row-label">{$t("view.settings.advanced.verboseLogs.label")}</div>
              <div class="row-sub">{$t("view.settings.advanced.verboseLogs.sub", { dir: "logs/" })}</div>
            </div>
            <label class="toggle">
              <input
                type="checkbox"
                checked={$settings.advanced.verbose_logs}
                aria-label={$t("view.settings.advanced.verboseLogs.label")}
                onchange={(e) => updateAdvanced("verbose_logs", (e.target as HTMLInputElement).checked)}
              />
              <span class="toggle-slider"></span>
            </label>
          </div>
          <div class="row row-divider">
            <div class="row-text">
              <div class="row-label">{$t("view.settings.advanced.preferStable.label")}</div>
              <div class="row-sub">{$t("view.settings.advanced.preferStable.sub")}</div>
            </div>
            <label class="toggle">
              <input
                type="checkbox"
                checked={$settings.advanced.prefer_stable_channel}
                aria-label={$t("view.settings.advanced.preferStable.label")}
                onchange={(e) => updateAdvanced("prefer_stable_channel", (e.target as HTMLInputElement).checked)}
              />
              <span class="toggle-slider"></span>
            </label>
          </div>
          <div class="row row-divider">
            <div class="row-text">
              <div class="row-label">
                {$t("view.settings.advanced.allowUnsigned.label")}
                <span class="chip chip-warning small-pill">{$t("view.settings.advanced.allowUnsigned.devOnly")}</span>
              </div>
              <div class="row-sub">{$t("view.settings.advanced.allowUnsigned.sub")}</div>
            </div>
            <label class="toggle">
              <input
                type="checkbox"
                checked={$settings.advanced.allow_unsigned_dlls}
                aria-label={$t("view.settings.advanced.allowUnsigned.label")}
                onchange={(e) => updateAdvanced("allow_unsigned_dlls", (e.target as HTMLInputElement).checked)}
              />
              <span class="toggle-slider"></span>
            </label>
          </div>
          <div class="row row-divider">
            <div class="row-text">
              <div class="row-label" id="adv-parallel-applies-label">{$t("view.settings.advanced.parallelApplies.label")}</div>
              <div class="row-sub" id="adv-parallel-applies-sub">{$t("view.settings.advanced.parallelApplies.sub")}</div>
            </div>
            <input
              type="number"
              class="inline-num"
              aria-labelledby="adv-parallel-applies-label"
              aria-describedby="adv-parallel-applies-sub"
              min="1"
              max="4"
              step="1"
              value={$settings.advanced.apply_concurrency}
              onchange={(e) => updateAdvanced("apply_concurrency", Math.max(1, Math.min(4, Number((e.target as HTMLInputElement).value) || 2)))}
            />
          </div>
        </div>

        <div class="settings-card">
          <header class="card-head">
            <h3>{$t("view.settings.advanced.network.title")}</h3>
            <p class="card-sub">{$t("view.settings.advanced.network.sub")}</p>
          </header>
          <div class="row">
            <div class="row-text">
              <div class="row-label" id="adv-network-retry-label">{$t("view.settings.advanced.network.retry.label")}</div>
              <div class="row-sub" id="adv-network-retry-sub">{$t("view.settings.advanced.network.retry.sub")}</div>
            </div>
            <input
              type="number"
              class="inline-num"
              aria-labelledby="adv-network-retry-label"
              aria-describedby="adv-network-retry-sub"
              min="1"
              max="6"
              step="1"
              value={$settings.network.retry_attempts}
              onchange={(e) => updateNetwork("retry_attempts", Math.max(1, Math.min(6, Number((e.target as HTMLInputElement).value) || 3)))}
            />
          </div>
          <div class="row row-divider">
            <div class="row-text">
              <div class="row-label" id="adv-network-chunk-timeout-label">{$t("view.settings.advanced.network.chunkTimeout.label")}</div>
              <div class="row-sub" id="adv-network-chunk-timeout-sub">{$t("view.settings.advanced.network.chunkTimeout.sub")}</div>
            </div>
            <input
              type="number"
              class="inline-num"
              aria-labelledby="adv-network-chunk-timeout-label"
              aria-describedby="adv-network-chunk-timeout-sub"
              min="10"
              max="600"
              step="10"
              value={$settings.network.chunk_timeout_secs}
              onchange={(e) => updateNetwork("chunk_timeout_secs", Math.max(10, Math.min(600, Number((e.target as HTMLInputElement).value) || 60)))}
            />
          </div>
          <div class="row row-divider">
            <div class="row-text">
              <div class="row-label" id="adv-network-connect-timeout-label">{$t("view.settings.advanced.network.connectTimeout.label")}</div>
              <div class="row-sub" id="adv-network-connect-timeout-sub">{$t("view.settings.advanced.network.connectTimeout.sub")}</div>
            </div>
            <input
              type="number"
              class="inline-num"
              aria-labelledby="adv-network-connect-timeout-label"
              aria-describedby="adv-network-connect-timeout-sub"
              min="3"
              max="60"
              step="1"
              value={$settings.network.connect_timeout_secs}
              onchange={(e) => updateNetwork("connect_timeout_secs", Math.max(3, Math.min(60, Number((e.target as HTMLInputElement).value) || 10)))}
            />
          </div>
          <div class="row row-divider">
            <div class="row-text">
              <div class="row-label" id="adv-network-cache-ttl-label">{$t("view.settings.advanced.network.cacheTtl.label")}</div>
              <div class="row-sub" id="adv-network-cache-ttl-sub">{$t("view.settings.advanced.network.cacheTtl.sub")}</div>
            </div>
            <input
              type="number"
              class="inline-num"
              aria-labelledby="adv-network-cache-ttl-label"
              aria-describedby="adv-network-cache-ttl-sub"
              min="60"
              max="3600"
              step="60"
              value={$settings.network.download_cache_ttl_secs}
              onchange={(e) => updateNetwork("download_cache_ttl_secs", Math.max(60, Math.min(3600, Number((e.target as HTMLInputElement).value) || 300)))}
            />
          </div>
        </div>

        <div class="settings-card">
          <header class="card-head">
            <h3>{$t("view.settings.advanced.dlssOverrides.title")} <span class="chip chip-update small-pill"><BrandMark key="nvidia" tone="mono" size={11} /></span></h3>
            <p class="card-sub">{$t("view.settings.advanced.dlssOverrides.sub", { drivers: $t("view.settings.advanced.dlssOverrides.driversTabBold") })}</p>
          </header>
          <button class="btn btn-primary card-cta" onclick={() => currentView.set("drivers")}>{$t("view.settings.advanced.dlssOverrides.openDrivers")}</button>
        </div>
      </section>
    {/if}
  </div>
  </div>
{/if}

<style>
  .view-header.settings-heading { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: start; gap: 24px; }
  .view-header { margin-bottom: var(--space-5); }
  .loading { padding: 60px 0; text-align: center; color: var(--text-muted); }


  @media (max-width: 640px) {

  }





  .hero-actions { display: flex; gap: var(--space-2); flex-wrap: wrap; }

  .settings-layout {
    display: grid;
    grid-template-columns: 220px minmax(0, 1fr);
    gap: clamp(var(--space-4), 2vw, var(--space-6));
    align-items: start;
    padding: var(--space-4);
    border-radius: var(--radius-xl);
    background: var(--bg-card);
    border: 1px solid var(--border);
    box-shadow: var(--shadow-xs);
  }
  @media (max-width: 900px) {
    .settings-layout { grid-template-columns: 1fr; gap: var(--space-4); padding: var(--space-3); }
  }
  .side-nav {
    position: sticky;
    top: var(--space-2);
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-right: var(--space-4);
    border-right: 1px solid var(--border);
  }
  @media (max-width: 900px) {
    .side-nav {
      position: static;
      flex-direction: row;
      flex-wrap: wrap;
      gap: var(--space-1);
      padding-right: 0;
      padding-bottom: var(--space-3);
      border-right: none;
      border-bottom: 1px solid var(--border);
    }
    .side-tab { flex: 1 1 auto; }
  }
  .side-tab {
    position: relative;
    display: grid;
    grid-template-columns: 22px 1fr;
    align-items: center;
    gap: var(--space-3);
    min-height: 40px;
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    color: var(--text-secondary);
    font-size: var(--fs-sm);
    font-weight: 600;
    text-align: left;
    cursor: pointer;
    border-radius: var(--radius-md);
    transition: background var(--dur-fast) var(--ease), color var(--dur-fast) var(--ease);
  }
  .side-tab:hover { background: var(--bg-elevated); color: var(--text-primary); }
  .side-tab:focus-visible { outline: none; box-shadow: var(--shadow-ring); }
  .side-tab-icon { color: var(--text-muted); display: inline-flex; transition: color var(--dur-fast) var(--ease); }
  .side-tab.active { background: var(--accent-dim); color: var(--accent); }
  .side-tab.active .side-tab-icon { color: var(--accent); }
  .side-tab-rail {
    position: absolute;
    left: 0;
    top: 50%;
    transform: translateY(-50%);
    width: 3px;
    height: 56%;
    border-radius: var(--radius-full);
    background: transparent;
    transition: background var(--dur-fast) var(--ease);
  }
  .side-tab.active .side-tab-rail { background: var(--accent); }

  .tab-panels { max-width: 960px; min-width: 0; container-type: inline-size; }
  .tab-panels > :global(section) {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .section-head {
    display: block;
    margin: 0;
    padding-left: var(--space-3);
    border-left: 2px solid var(--accent);
  }
  .section-head-gap { margin-top: var(--space-5); }
  .section-title-h {
    font-size: var(--fs-md);
    font-weight: 700;
    letter-spacing: var(--letter-tight);
    color: var(--text-primary);
    margin-bottom: 2px;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    text-transform: none;
  }
  .section-tag {
    font-size: var(--fs-2xs);
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: var(--letter-wider);
    padding: 1px 7px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius-full);
    font-weight: 600;
  }
  .section-help {
    font-size: var(--fs-sm);
    color: var(--text-secondary);
    line-height: var(--lh-snug);
    max-width: 64ch;
  }
  .section-help.muted { font-size: var(--fs-xs); color: var(--text-muted); margin-top: var(--space-1); }

  .card { padding: var(--space-1) var(--space-5); }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--space-5);
    padding: var(--space-4) 0;
  }
  .art-row {
    align-items: start;
    grid-template-columns: minmax(0, 1fr) clamp(200px, 38%, 300px);
    gap: var(--space-4);
  }
  @container (max-width: 520px) {
    .art-row { grid-template-columns: 1fr; gap: var(--space-2); }
  }
  .row-divider { border-top: 1px solid var(--border); }
  .row-disabled { opacity: 0.5; }
  .row-disabled .row-label,
  .row-disabled .row-sub { color: var(--text-muted); }
  .interval-select {
    height: 36px;
    padding: 0 var(--space-3);
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    border: 1px solid var(--border);
    color: var(--text-primary);
    font-size: var(--fs-sm);
    font-family: inherit;
    font-variant-numeric: tabular-nums;
    cursor: pointer;
    min-width: 9rem;
    flex-shrink: 0;
    transition: border-color var(--dur-fast) var(--ease), box-shadow var(--dur-fast) var(--ease);
  }
  .interval-select:hover:not(:disabled) { border-color: var(--border-hover); }
  .interval-select:focus-visible { outline: none; border-color: var(--accent); box-shadow: var(--shadow-ring); }
  .interval-select:disabled { cursor: not-allowed; }
  .inline-num {
    width: 88px;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--bg-input);
    color: var(--text-primary);
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
    font-variant-numeric: tabular-nums;
    text-align: right;
    flex-shrink: 0;
    transition: border-color var(--dur-fast) var(--ease), box-shadow var(--dur-fast) var(--ease);
  }
  .inline-num:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-dim);
  }
  .lang-select { width: 200px; max-width: 100%; flex-shrink: 0; }
  .art-row input[type="text"],
  .art-row input[type="password"] {
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--bg-input);
    color: var(--text-primary);
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
    transition: border-color var(--dur-fast) var(--ease), box-shadow var(--dur-fast) var(--ease);
  }
  .art-row input::placeholder { color: var(--text-placeholder); }
  .art-row input:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-dim);
  }
  .row-label { font-size: var(--fs-base); font-weight: 600; color: var(--text-primary); display: inline-flex; align-items: center; gap: var(--space-2); flex-wrap: wrap; }
  .row-sub { font-size: var(--fs-xs); color: var(--text-secondary); margin-top: 3px; line-height: var(--lh-normal); }
  .row-sub-caution { color: var(--warning); }
  .update-status { font-weight: 500; }
  .update-status-caution { color: var(--warning); }
  .row-text { min-width: 0; }
  .settings-card {
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-1) var(--space-5) var(--space-5);
    min-width: 0;
  }
  .card-head {
    padding: var(--space-4) 0 var(--space-2);
    margin-bottom: var(--space-2);
    border-bottom: 1px solid var(--border);
  }
  .card-head h3 {
    font-size: var(--fs-base);
    font-weight: 700;
    color: var(--text-primary);
    letter-spacing: var(--letter-tight);
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .card-sub { font-size: var(--fs-xs); color: var(--text-secondary); margin-top: 3px; line-height: var(--lh-snug); }
  .card-cta { margin-top: var(--space-4); }

  .feature-group { display: flex; flex-direction: column; gap: var(--space-2); }
  .feature-group + .feature-group { margin-top: var(--space-4); }
  .feature-group-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding-left: var(--space-1);
  }
  .feature-group-logo {
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-md);
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    flex-shrink: 0;
  }
  .feature-group-label {
    font-size: var(--fs-2xs);
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: var(--letter-wider);
    color: var(--text-secondary);
  }

  .files-disclosure {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-top: var(--space-2);
    font-size: var(--fs-2xs);
    font-weight: 600;
    color: var(--text-secondary);
    background: transparent;
    border: none;
    cursor: pointer;
    padding: 2px 0;
    border-radius: var(--radius-xs);
    transition: color var(--dur-fast) var(--ease);
  }
  .files-disclosure:hover { color: var(--accent); }
  .files-disclosure:focus-visible { outline: none; box-shadow: var(--shadow-ring); }
  .files-disclosure .chev { transition: transform var(--dur-fast) var(--ease); }
  .files-disclosure .chev.open { transform: rotate(180deg); }
  .files-meta { margin-top: var(--space-2); padding: var(--space-2) var(--space-3); background: var(--bg-input); border: 1px solid var(--border); border-radius: var(--radius-sm); font-size: var(--fs-2xs); color: var(--text-secondary); }

  .folder-input-row { display: flex; gap: var(--space-3); align-items: center; padding: var(--space-4) 0 var(--space-1); flex-wrap: wrap; }
  .folder-input-wrap {
    flex: 1 1 240px;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    height: 42px;
    padding: 0 var(--space-4);
    background: var(--bg-elevated);
    border-radius: var(--radius-full);
    border: 1px solid var(--border);
    transition: border-color var(--dur-fast) var(--ease), background var(--dur-fast) var(--ease), box-shadow var(--dur-fast) var(--ease);
  }
  .folder-input-wrap:focus-within {
    border-color: var(--accent);
    background: var(--bg-card);
    box-shadow: 0 0 0 4px var(--accent-soft);
  }
  .folder-input-icon { color: var(--text-muted); flex-shrink: 0; }
  .folder-input-wrap input {
    flex: 1;
    min-width: 0;
    border: none;
    background: transparent;
    padding: 0;
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
    color: var(--text-primary);
  }
  .folder-input-wrap input:focus { outline: none; }
  .folder-input-wrap input::placeholder { color: var(--text-placeholder); }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-6) 0 var(--space-5);
    color: var(--text-muted);
    font-size: var(--fs-sm);
  }

  .path-list { list-style: none; padding: var(--space-3) 0 var(--space-1); margin: 0; display: flex; flex-direction: column; gap: var(--space-2); }
  .path-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    transition: background var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease);
  }
  .path-row:hover { background: var(--bg-card-hover); border-color: var(--border-hover); }
  .path-icon { width: 32px; height: 32px; border-radius: var(--radius-md); }
  .path-text { font-size: var(--fs-xs); color: var(--text-primary); flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .path-action {
    width: 32px;
    height: 32px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-md);
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    flex-shrink: 0;
    transition: background 0.12s var(--ease), color 0.12s var(--ease);
  }
  .path-action:hover { background: var(--bg-card-hover); color: var(--text-primary); }
  .path-action:focus-visible { outline: none; box-shadow: var(--shadow-ring); }
  .path-action.path-action-danger:hover { background: var(--badge-red-bg); color: var(--badge-red-fg); }
  .row-actions { padding: var(--space-3) 0 var(--space-4); }

  .launcher-card { padding: var(--space-2) var(--space-5); }
  .launcher-row {
    display: grid;
    grid-template-columns: 200px minmax(0, 1fr);
    gap: var(--space-5);
    align-items: start;
    padding: var(--space-4) 0;
  }
  @container (max-width: 560px) {
    .launcher-row { grid-template-columns: 1fr; gap: var(--space-3); }
  }
  .launcher-head { display: flex; align-items: center; gap: var(--space-3); min-width: 0; }
  .launcher-logo {
    width: 40px;
    height: 40px;
    border-radius: 11px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    box-shadow: var(--shadow-xs), inset 0 0 0 1px rgba(255, 255, 255, 0.08);
  }
  .launcher-logo svg { display: block; }
  .launcher-head-text { min-width: 0; }
  .launcher-input-col { display: flex; flex-direction: column; gap: var(--space-2); min-width: 0; }
  .path-input-row { display: flex; align-items: stretch; gap: var(--space-2); height: 38px; }
  .path-input-row input {
    flex: 1;
    min-width: 0;
    padding: 0 var(--space-3);
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    color: var(--text-primary);
    transition: border-color var(--dur-fast) var(--ease), background var(--dur-fast) var(--ease), box-shadow var(--dur-fast) var(--ease);
  }
  .path-input-row input:focus {
    outline: none;
    border-color: var(--accent);
    background: var(--bg-card);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .path-remove {
    width: 36px;
    flex-shrink: 0;
    border-radius: var(--radius-md);
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    transition: background 0.12s var(--ease), color 0.12s var(--ease);
  }
  .path-remove:hover { background: var(--badge-red-bg); color: var(--badge-red-fg); }
  .path-remove:focus-visible { outline: none; box-shadow: var(--shadow-ring); }
  .add-path-pill {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 14px;
    border-radius: var(--radius-full);
    background: transparent;
    border: 1px dashed var(--border-strong);
    color: var(--text-muted);
    font-size: var(--fs-xs);
    font-weight: 600;
    cursor: pointer;
    transition: background 0.12s var(--ease), color 0.12s var(--ease), border-color 0.12s var(--ease);
  }
  .add-path-pill:hover {
    background: var(--accent-soft);
    color: var(--accent);
    border-color: var(--accent);
    border-style: solid;
  }
  .add-path-pill:focus-visible { outline: none; box-shadow: var(--shadow-ring); }

  @media (prefers-reduced-motion: reduce) {
    .side-tab, .side-tab-icon, .side-tab-rail, .inline-num, .files-disclosure,
    .files-disclosure .chev, .folder-input-wrap, .path-row, .path-action,
    .path-remove, .path-input-row input, .add-path-pill, .interval-select { transition: none; }
  }

  .settings-files { position: relative; align-self: flex-start; margin-left: auto; }
  .settings-files > summary { display: flex; align-items: center; gap: 12px; list-style: none; padding: 10px 14px; border: 1px solid var(--border); border-radius: 8px; cursor: pointer; font-size: 13px; color: var(--text-secondary); }
  .settings-files > summary::-webkit-details-marker { display: none; }
  .settings-files[open] { min-width: min(100%, 290px); padding: 12px; border: 1px solid var(--border); border-radius: 10px; background: var(--bg-card); }
  .settings-files[open] > summary { padding: 0 0 12px; border: 0; border-radius: 0; justify-content: space-between; }
  .settings-files .hero-actions { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; }
  .settings-file-meta { font-size: 12px; color: var(--text-muted); margin: 12px 0 0; }
  .settings-section-picker { display: none; margin-bottom: 28px; }
  .settings-section-picker > span { font-size: 12px; color: var(--text-secondary); }
  .settings-section-picker > select { width: 100%; min-height: 44px; padding: 10px 12px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg-input); color: var(--text-primary); font: inherit; }
  .settings-layout { grid-template-columns: 180px minmax(0, 1fr); gap: 36px; padding: 0; border: 0; border-radius: 0; background: none; box-shadow: none; }
  .settings-layout .side-nav { flex-direction: column; padding: 0 20px 0 0; border: 0; border-right: 1px solid var(--border); }
  .tab-panels { width: 100%; min-width: 0; }
  .tab-panels .section-head { padding: 0; border: 0; }
  .section-title-h { font-size: 20px; line-height: 1.4; margin-bottom: 6px; }
  .section-help { font-size: 13px; line-height: 1.55; }
  .tab-panels .card { padding: 0; border: 0; background: none; box-shadow: none; }
  .tab-panels .row { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: start; gap: 24px; padding: 22px 0; }
  .tab-panels .row-text { min-width: 0; }
  .tab-panels .row-label { font-size: 14px; line-height: 1.45; }
  .tab-panels .row-sub { font-size: 13px; line-height: 1.55; max-width: 64ch; margin-top: 5px; }
  .tab-panels .row > .toggle { margin-top: 2px; }

  .setting-detail { margin-top: 10px; color: var(--text-muted); font-size: 12px; line-height: 1.5; }
  .setting-detail summary { cursor: pointer; width: fit-content; }
  .setting-detail p { margin: 8px 0 0; overflow-wrap: anywhere; }
  @container workspace (max-width: 760px) {
    .settings-layout { display: block; }
    .settings-layout .side-nav { display: none; }
    .settings-section-picker { display: grid; gap: 8px; }
    .tab-panels .row { gap: 20px; }
  }
  @container workspace (max-width: 420px) {
    .tab-panels .art-row { grid-template-columns: minmax(0, 1fr); }
  }

  .feature-group > summary { cursor: pointer; padding: 16px 0; }
  .feature-group:not([open]) { border-bottom: 1px solid var(--border); }
  .feature-group-logo { width: auto; min-width: 30px; }
</style>
