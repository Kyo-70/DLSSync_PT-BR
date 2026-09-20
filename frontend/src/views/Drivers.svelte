<script lang="ts">
  import { onMount } from "svelte";
  import { slide } from "svelte/transition";
  import { get } from "svelte/store";
  import { openUrl, systemDriverVersions, type DriverStatusReport, type GpuVendor } from "../lib/api";
  import { isNexusBuild } from "../lib/distribution";
  import { supportsWindowsDrivers } from "../lib/platform";
  import { t, locale, translate } from "../lib/i18n/index";
  import {
    driverStatusTone,
    sortDriverReports,
    driverPageUrl,
    driverHealth,
    driverCardState,
    installProgressView,
  } from "../lib/drivers";
  import {
    driverReports,
    systemInfo,
    ensureSystemInfo,
    driverRebootPending,
    driverCheckInProgress,
    driverCheckError,
    loadDriverUpdates,
    startDriverInstall,
    driverInstall,
    showToast,
    systemDriverGroups,
    systemScanInProgress,
    systemScanError,
    systemScanRan,
    loadSystemDrivers,
    startSystemDriverInstall,
    systemDriverInstall,
    DRIVER_INSTALL_STAGE_LABEL,
  } from "../lib/stores";
  import type { SystemDriverUpdate, SystemDeviceClass, DriverStoreVersion } from "../lib/api";
  import { formatBytes } from "../lib/formatHuman";
  import DlssOverridePanel from "../components/DlssOverridePanel.svelte";
  import DeviceInventory from "../components/DeviceInventory.svelte";
  import DriverHistoryFlyout from "../components/DriverHistoryFlyout.svelte";
  import BrandMark from "../components/BrandMark.svelte";
  import { BRANDS } from "../lib/brands";

  let activeSection = $state<"graphics" | "system" | "profiles">("graphics");
  function goToSection(section: "graphics" | "system" | "profiles"): void {
    activeSection = section;
    document.getElementById(`drivers-${section}`)?.scrollIntoView({ block: "start", behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "instant" : "smooth" });
  }
  let expandedModel = $state<string | null>(null);
  let systemDetailsOpen = $state(false);
  let historyTarget = $state<{ vendor: GpuVendor; model: string; accent: string } | null>(null);

  const VENDOR_ACCENT: Record<GpuVendor, string> = {
    nvidia: "var(--vendor-nvidia)",
    amd: "var(--vendor-amd)",
    intel: "var(--vendor-intel)",
    other: "var(--neutral)",
  };

  let reports = $derived(sortDriverReports($driverReports));
  let pendingReboots = $derived($driverRebootPending);
  let installBusy = $derived($driverInstall.vendor !== null);
  let nvidiaPacked = $derived(
    $driverReports.find((r) => r.device.vendor === "nvidia")?.installed.packed ?? 0,
  );
  let hasNvidia = $derived($driverReports.some((r) => r.device.vendor === "nvidia") || ($systemInfo?.gpus ?? []).some(gpu => gpu.vendor === "nvidia"));
  // 10.06: the update count and the health semantics are projected from the backend `UpdateStatus`
  // enum in one place. The view no longer compares status strings or invents a state for a status
  // it does not recognise.

  let nonNvidia = $derived(
    $driverReports
      .map((r) => r.device.vendor)
      .filter((v) => v === "amd" || v === "intel") as ("amd" | "intel")[],
  );

  function vendorLabel(vendor: GpuVendor): string {
    return vendor === "other" ? "GPU" : BRANDS[vendor].label;
  }

  function sizeLabel(bytes: number): string {
    if (bytes <= 0) return "";
    const mb = bytes / (1024 * 1024);
    return mb >= 1024 ? `${(mb / 1024).toFixed(2)} GB` : `${mb.toFixed(0)} MB`;
  }

  function hasChangelog(report: DriverStatusReport): boolean {
    const log = report.latest?.changelog;
    return !!log && ((log.highlights?.length ?? 0) > 0 || (log.fixed?.length ?? 0) > 0);
  }

  async function open(url: string | null): Promise<void> {
    if (!url) return;
    try {
      await openUrl(url);
    } catch (err) {
      showToast("warning", translate(get(locale), "view.drivers.openLinkFailed", { error: String(err) }));
    }
  }

  function toggleChangelog(model: string): void {
    expandedModel = expandedModel === model ? null : model;
  }

  let systemUpdateCount = $derived(
    $systemDriverGroups.reduce((n, g) => n + g.updates.length, 0),
  );
  let systemBusy = $derived(
    $systemDriverInstall.updateId !== null && $systemDriverInstall.stage !== "failed",
  );

  function isInstalling(update: SystemDriverUpdate): boolean {
    return $systemDriverInstall.updateId === update.update_id;
  }

  // DriverStore version history (current + superseded), lazily loaded per card.
  type VersionState = DriverStoreVersion[] | "loading" | "error";
  let versionsByUpdate = $state<Record<string, VersionState>>({});

  async function toggleVersions(update: SystemDriverUpdate): Promise<void> {
    const id = update.update_id;
    if (versionsByUpdate[id]) {
      const next = { ...versionsByUpdate };
      delete next[id];
      versionsByUpdate = next;
      return;
    }
    if (!update.target_inf) return;
    versionsByUpdate = { ...versionsByUpdate, [id]: "loading" };
    try {
      const list = await systemDriverVersions(update.target_inf);
      versionsByUpdate = { ...versionsByUpdate, [id]: list };
    } catch {
      versionsByUpdate = { ...versionsByUpdate, [id]: "error" };
    }
  }

  // Per-class icon (24x24 stroke paths) for the System & Components section headers.
  const CLASS_ICON: Record<SystemDeviceClass, string> = {
    audio: "M11 5 6 9H2v6h4l5 4zM15.54 8.46a5 5 0 0 1 0 7.07M19.07 4.93a10 10 0 0 1 0 14.14",
    display: "M2 4h20v12H2zM8 20h8M12 16v4",
    monitor: "M2 3h20v14H2zM8 21h8M12 17v4",
    network: "M5 12.55a11 11 0 0 1 14 0M1.42 9a16 16 0 0 1 21.16 0M8.53 16.11a6 6 0 0 1 6.95 0M12 20h.01",
    bluetooth: "m7 7 10 10-5 5V2l5 5L7 17",
    input: "M6 8h.01M10 8h.01M14 8h.01M18 8h.01M8 12h.01M12 12h.01M16 12h.01M7 16h10M2 5h20v14H2z",
    storage: "M22 12H2M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11zM6 16h.01M10 16h.01",
    printer: "M6 9V2h12v7M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2M6 14h12v8H6z",
    camera: "M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2zM12 17a4 4 0 1 0 0-8 4 4 0 0 0 0 8z",
    sensor: "M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20zM12 6a6 6 0 1 0 0 12 6 6 0 0 0 0-12zM12 10a2 2 0 1 0 0 4 2 2 0 0 0 0-4z",
    battery: "M3 7h16v10H3zM19 10h2v4h-2M7 10v4M11 10v4",
    smart_card: "M2 5h20v14H2zM2 9h20M6 14h6",
    firmware: "M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3M5 5h14v14H5zM9 9h6v6H9z",
    chipset: "M6 6h12v12H6zM9 2v2M15 2v2M9 20v2M15 20v2M2 9h2M2 15h2M20 9h2M20 15h2M10 10h4v4h-4z",
    system: "M4 4h16v16H4zM9 9h6v6H9zM9 1v3M15 1v3M9 20v3M15 20v3M20 9h3M20 14h3M1 9h3M1 14h3",
    usb: "M12 2v16M12 18a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM8 8l4-4 4 4M7 12l-3 3 3 3M5 15h7",
    other: "M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20zM12 16v-4M12 8h.01",
  };

  // Strip the redundant " Driver Update (x.x.x)" / "(x.x.x.x)" tail WUA appends.
  function cleanTitle(title: string): string {
    return title
      .replace(/\s*Driver Update\s*\([^)]*\)\s*$/i, "")
      .replace(/\s*\(\d+(\.\d+){1,3}\)\s*$/, "")
      .replace(/\s*-\s*\d{1,2}\/\d{1,2}\/\d{4}.*$/i, "")
      .trim();
  }

  function deviceName(update: SystemDriverUpdate): string {
    return update.target_device ?? cleanTitle(update.title);
  }

  // WUA's SupportUrl is often a generic 404 hub; the backend already filters
  // that out, so when there's no real vendor URL we link to the exact package
  // in the Microsoft Update Catalog (always resolves).
  const UPDATE_CATALOG_SEARCH = "https://www.catalog.update.microsoft.com/Search.aspx?q=";
  function vendorLink(update: SystemDriverUpdate): string {
    return update.support_url ?? `${UPDATE_CATALOG_SEARCH}${encodeURIComponent(update.title)}`;
  }

  onMount(() => {
    if (!supportsWindowsDrivers) return;
    void ensureSystemInfo().catch(() => undefined);
    if (!isNexusBuild) {
      void loadDriverUpdates();
      void loadSystemDrivers();
    }
  });
</script>

<section class="drivers-view">
  <header class="view-header drivers-heading">
    <div>
      <h1 class="view-title">{$t("view.drivers.title")}</h1>
      <p class="view-subtitle">
        {$t(supportsWindowsDrivers ? "view.drivers.subtitle" : "view.drivers.windowsOnly")}
      </p>
    </div>
    {#if supportsWindowsDrivers}<div class="header-actions">
      <button class="check-btn" onclick={() => loadDriverUpdates()} disabled={$driverCheckInProgress}>
        {$driverCheckInProgress ? $t("view.drivers.checking") : $t("view.drivers.checkForUpdates")}
      </button>
    </div>{/if}
  </header>

  {#if supportsWindowsDrivers}
  <nav class="drivers-tabs" aria-label={$t("view.drivers.title")}>
    <button data-testid="drivers-tab-graphics" class:active={activeSection === "graphics"} aria-pressed={activeSection === "graphics"} onclick={() => goToSection("graphics")}>{$t("view.drivers.graphics")}<span>{reports.length}</span></button>
    <button data-testid="drivers-tab-system" class:active={activeSection === "system"} aria-pressed={activeSection === "system"} onclick={() => goToSection("system")}>{$t("view.drivers.devices")}<span>{systemUpdateCount}</span></button>
    {#if hasNvidia}<button data-testid="drivers-tab-profiles" class:active={activeSection === "profiles"} aria-pressed={activeSection === "profiles"} onclick={() => goToSection("profiles")}>{$t("view.drivers.dlssOverrides")}</button>{/if}
  </nav>

  {#if $driverCheckError}
    <p class="error-banner">{$driverCheckError}</p>
  {/if}

  {#if reports.length === 0 && !$driverCheckInProgress && $systemInfo && $systemInfo.gpus.length === 0}
    <p class="empty">{$t("view.drivers.noGpus")}</p>
  {/if}

  <div class="driver-section" id="drivers-graphics">
  {#if reports.length === 0 && $systemInfo && $systemInfo.gpus.length > 0}
    <ul class="driver-list">
      {#each $systemInfo.gpus as gpu (`${gpu.vendor}:${gpu.pci_device_id}:${gpu.model}`)}
        <li class="driver-card" data-health="unchecked">
          <div class="card-main">
            <BrandMark key={gpu.vendor} fit="wordmark" size={24} />
            <div class="model-block"><span class="model">{gpu.model}</span><span class="versions mono">{gpu.driver_version}</span></div>
            <span class="driver-state" data-tone="neutral">{$driverCheckInProgress ? $t("view.drivers.checking") : $t("view.drivers.notChecked")}</span>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
  <ul class="driver-list">
    {#each reports as report (report.device.model)}
      {@const tone = driverStatusTone(report.status)}
      {@const expanded = expandedModel === report.device.model}
      {@const pageUrl = driverPageUrl(report)}
      {@const showNotes = !!pageUrl}
      {@const installing = $driverInstall.vendor === report.device.vendor}
      {@const cardState = driverCardState(report, { installing, rebootPendingVersion: pendingReboots[report.device.vendor] })}
      <li class="driver-card" data-health={driverHealth(report)}>
        <div class="card-row">
          <div class="card-main">
            <span class="vendor-pill" data-vendor={report.device.vendor}>
              {#if report.device.vendor === "other"}
                {vendorLabel(report.device.vendor)}
              {:else}
                <BrandMark key={report.device.vendor} fit="wordmark" size={24} />
              {/if}
            </span>
            <div class="model-block">
              <span class="model">{report.device.model}</span>
              <span class="versions mono">
                {report.installed.display}
                {#if report.status === "update_available" && report.latest}
                  <span class="arrow">→</span>
                  <span class="next">{report.latest.version.display}</span>
                  {#if report.latest.is_beta}<span class="chan-chip beta">{$t("view.drivers.channelBeta")}</span>{:else}<span class="chan-chip whql">{$t("view.drivers.channelWhql")}</span>{/if}
                {/if}
              </span>
            </div>
          </div>
          <div class="card-side">
            {#if cardState.kind === "installing"}
              {@const progress = installProgressView(
                $driverInstall.stage,
                $driverInstall.fraction,
                $driverInstall.message,
              )}
              <div
                class="install-live"
                class:is-failed={progress.kind === "failed"}
                data-testid="gpu-install-live"
                data-progress={progress.kind}
                role="status"
                aria-live="polite"
              >
                <span class="install-stage">
                  {$t(`installStage.${$driverInstall.stage ?? ""}`)}
                </span>
                {#if progress.kind === "determinate"}
                  <div class="install-bar" data-testid="gpu-install-bar">
                    <div class="install-fill" style:width={`${progress.percent}%`}></div>
                  </div>
                {:else if progress.kind === "indeterminate"}
                  <div class="install-bar indeterminate" data-testid="gpu-install-bar">
                    <div class="install-fill"></div>
                  </div>
                {:else}
                  <span class="install-failed-note" data-testid="gpu-install-failed">
                    {$t("view.drivers.installFailedNote")}
                  </span>
                {/if}
                <span class="install-msg">{progress.message}</span>
              </div>
            {:else}
              {#if cardState.kind === "reboot_pending"}
                <span class="driver-state reboot-pending" data-tone="warning" data-testid="gpu-reboot-pending" title={$t("view.drivers.restartPending", { version: cardState.version })}>
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/></svg>
                  {$t("view.drivers.restartToFinish")}
                </span>
              {:else if cardState.action.kind === "install"}
                {@const size = sizeLabel(cardState.action.size_bytes)}
                <button class="driver-update" data-testid="gpu-install-action" onclick={() => startDriverInstall(report)} disabled={installBusy}>
                  <span class="driver-update-label">{$t("view.drivers.updateTo", { version: report.latest?.version.display ?? "" })}</span>
                  {#if size}<span class="driver-update-size mono">{size}</span>{/if}
                </button>
              {:else if cardState.action.kind === "open_page"}
                {@const officialPage = cardState.action.url}
                <button class="driver-update open-page" data-testid="gpu-open-page-action" onclick={() => open(officialPage)} disabled={installBusy}>
                  <span class="driver-update-label">{$t("view.drivers.openDownloadPage")}</span>
                  <span class="ext-arrow" aria-hidden="true">↗</span>
                </button>
              {:else}
                {@const helpUrl = cardState.action.help_url}
                <div class="state-block" data-testid="gpu-status-only">
                  <span class="driver-state" data-tone={tone}>{$t("driverStatus." + report.status)}</span>
                  {#if helpUrl}
                    <button class="help-link" onclick={() => open(helpUrl)}>{$t("view.drivers.findMyDriver")}</button>
                  {/if}
                </div>
              {/if}
              <div class="driver-secondary">
                {#if showNotes}
                  <button
                    class="driver-icon"
                    onclick={() => open(pageUrl)}
                    title={$t("view.drivers.releaseNotesTitle")}
                    aria-label={$t("view.drivers.releaseNotesAria")}
                  >
                    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
                  </button>
                {/if}
                <button
                  class="driver-icon"
                  onclick={() => (historyTarget = { vendor: report.device.vendor, model: report.device.model, accent: VENDOR_ACCENT[report.device.vendor] })}
                  title={$t("view.drivers.allVersionsTitle")}
                  aria-label={$t("view.drivers.allVersionsAria")}
                >
                  <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="12 2 2 7 12 12 22 7 12 2"/><polyline points="2 17 12 22 22 17"/><polyline points="2 12 12 17 22 12"/></svg>
                </button>
              </div>
            {/if}
          </div>
        </div>

        {#if hasChangelog(report) || showNotes}
          <button class="changelog-toggle" onclick={() => toggleChangelog(report.device.model)} aria-expanded={expanded}>
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" class="chev" class:open={expanded}><polyline points="6 9 12 15 18 9"/></svg>
            {$t("view.drivers.whatsNew")}
          </button>
          {#if expanded}
            <div class="changelog">
              {#if hasChangelog(report) && report.latest?.changelog}
                {#if (report.latest.changelog.highlights?.length ?? 0) > 0}
                  <ul class="cl-highlights">
                    {#each (report.latest.changelog.highlights ?? []) as h}<li>{h}</li>{/each}
                  </ul>
                {/if}
                {#if (report.latest.changelog.fixed?.length ?? 0) > 0}
                  <span class="cl-label">{$t("view.drivers.changelogFixed")}</span>
                  <ul class="cl-fixed">
                    {#each (report.latest.changelog.fixed ?? []) as f}<li>{f}</li>{/each}
                  </ul>
                {/if}
              {:else}
                <p class="cl-empty">{$t("view.drivers.noInlineNotes")}</p>
              {/if}
              {#if showNotes}
                <button class="link-btn inline" onclick={() => open(pageUrl)}>{$t("view.drivers.fullReleaseNotes")}</button>
              {/if}
            </div>
          {/if}
        {/if}
      </li>
    {/each}
  </ul>

  </div>

  <section class="feature-block system-block" id="drivers-system">
    <div class="section-head feature-section-head">
      <span class="section-title">{$t("view.drivers.systemComponents")}</span>
      <span class="beta-tag">Windows Update</span>
      {#if systemUpdateCount > 0}
        <span class="section-count">{systemUpdateCount}</span>
      {/if}
      <button class="check-btn ghost" onclick={() => loadSystemDrivers()} disabled={$systemScanInProgress}>
        {$systemScanInProgress ? $t("view.drivers.scanning") : $t("view.drivers.rescan")}
      </button>
    </div>
    <div class="disclosure">
      <p class="feature-sub disclosure-summary">{$t("view.drivers.systemSummary")}</p>
      <button
        class="disclosure-toggle"
        onclick={() => (systemDetailsOpen = !systemDetailsOpen)}
        aria-expanded={systemDetailsOpen}
        aria-controls="system-details"
      >
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" class="chev" class:open={systemDetailsOpen} aria-hidden="true"><polyline points="6 9 12 15 18 9"/></svg>
        {systemDetailsOpen ? $t("view.drivers.showLess") : $t("view.drivers.learnMore")}
      </button>
      {#if systemDetailsOpen}
        <div id="system-details" class="disclosure-detail" transition:slide={{ duration: 160 }}>
          <p class="feature-sub">{$t("view.drivers.systemSub")}</p>
          <p class="beta-note">{$t("view.drivers.betaNote")}</p>
          <p class="admin-note">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="11" width="18" height="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>
            <span>{$t("note.adminElevation")}</span>
          </p>
        </div>
      {/if}
    </div>

    {#if $systemScanError}
      <p class="error-banner edge-accent is-danger">{$systemScanError}</p>
    {/if}

    {#if $systemScanInProgress && systemUpdateCount === 0}
      <div class="sys-scanning" role="status" aria-live="polite">
        <p class="sys-scanning-label">
          <span class="spin" aria-hidden="true"></span>
          {$t("view.drivers.scanningWindowsUpdate")}
        </p>
        <ul class="sys-skeletons" aria-hidden="true">
          {#each [0, 1, 2] as i (i)}
            <li class="sys-skeleton-card">
              <span class="skeleton skel-icon"></span>
              <span class="skel-lines">
                <span class="skeleton skel-line skel-line-wide"></span>
                <span class="skeleton skel-line skel-line-narrow"></span>
              </span>
              <span class="skeleton skel-btn"></span>
            </li>
          {/each}
        </ul>
      </div>
    {:else if $systemScanRan && systemUpdateCount === 0 && !$systemScanError}
      <p class="empty">{$t("view.drivers.allComponentsUpToDate")}</p>
    {/if}

    <div class="sys-groups">
      {#each $systemDriverGroups as group (group.class)}
        <section class="sys-group">
          <header class="sys-group-head">
            <span class="sys-group-icon" aria-hidden="true">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d={CLASS_ICON[group.class]} /></svg>
            </span>
            <span class="sys-group-label">{group.label}</span>
            <span class="sys-group-count">{group.updates.length}</span>
          </header>
          <ul class="sys-list">
            {#each group.updates as update (update.update_id)}
              {@const installing = isInstalling(update)}
              {@const versions = versionsByUpdate[update.update_id]}
              <li class="sys-card" class:is-installing={installing}>
                <div class="sys-row">
                <div class="sys-main">
                  <span class="sys-name" title={update.title}>{deviceName(update)}</span>
                  <span class="sys-meta">
                    <span class="sys-provider"><BrandMark key={update.provider} size={12} /></span>
                    <span class="sys-versions mono">
                      {#if update.current_version}<span class="sys-cur">{update.current_version}</span><span class="sys-arrow">→</span>{/if}
                      <span class="sys-new">{update.driver_version ?? $t("view.drivers.latestFallback")}</span>
                    </span>
                    {#if update.driver_date}<span class="sys-dot">·</span><span class="sys-date mono">{update.driver_date}</span>{/if}
                  </span>
                </div>
                <div class="sys-side">
                  {#if installing}
                    <!-- 10.08: a failed stage is presented as a failure. It carries no fraction, so
                         no progress bar and no fabricated 100 % width are rendered for it. -->
                    {@const progress = installProgressView(
                      $systemDriverInstall.stage,
                      $systemDriverInstall.fraction,
                      $systemDriverInstall.message,
                    )}
                    <div
                      class="install-live"
                      class:is-failed={progress.kind === "failed"}
                      data-testid="sys-install-live"
                      data-progress={progress.kind}
                      role="status"
                      aria-live="polite"
                    >
                      <span class="install-stage">
                        {DRIVER_INSTALL_STAGE_LABEL[$systemDriverInstall.stage ?? ""] ?? $t("view.drivers.installStageFallback")}
                      </span>
                      {#if progress.kind === "determinate"}
                        <div class="install-bar" data-testid="sys-install-bar">
                          <div class="install-fill" style:width={`${progress.percent}%`}></div>
                        </div>
                      {:else if progress.kind === "indeterminate"}
                        <div class="install-bar indeterminate" data-testid="sys-install-bar">
                          <div class="install-fill"></div>
                        </div>
                      {:else}
                        <span class="install-failed-note" data-testid="sys-install-failed">
                          {$t("view.drivers.installFailedNote")}
                        </span>
                      {/if}
                      <span class="install-msg">{progress.message}</span>
                    </div>
                  {:else}
                    {#if update.target_inf}
                      <button
                        class="driver-icon"
                        class:is-on={versions !== undefined}
                        onclick={() => toggleVersions(update)}
                        title={$t("view.drivers.versionHistoryTitle")}
                        aria-label={$t("view.drivers.versionHistoryAria")}
                        aria-expanded={versions !== undefined}
                      >
                        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 3v5h5"/><path d="M3.05 13A9 9 0 1 0 6 5.3L3 8"/><path d="M12 7v5l4 2"/></svg>
                      </button>
                    {/if}
                    <button
                      class="driver-icon"
                      data-href={vendorLink(update)}
                      onclick={() => open(vendorLink(update))}
                      title={update.support_url ? $t("view.drivers.vendorInfoTitle") : $t("view.drivers.catalogInfoTitle")}
                      aria-label={$t("view.drivers.driverInfoAria")}
                    >
                      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
                    </button>
                    <button class="driver-update" onclick={() => startSystemDriverInstall(update, group.label)} disabled={systemBusy}>
                      <span class="driver-update-label">{$t("view.drivers.update")}</span>
                      {#if update.size_bytes > 0}<span class="driver-update-size mono">{formatBytes(update.size_bytes)}</span>{/if}
                    </button>
                  {/if}
                </div>
                </div>
                {#if versions !== undefined}
                  <div class="sys-versions-panel" transition:slide={{ duration: 160 }}>
                    {#if versions === "loading"}
                      <span class="sys-versions-msg">{$t("view.drivers.readingDriverStore")}</span>
                    {:else if versions === "error"}
                      <span class="sys-versions-msg is-error">{$t("view.drivers.readVersionsError")}</span>
                    {:else if versions.length === 0}
                      <span class="sys-versions-msg">{$t("view.drivers.noCachedVersions", { version: update.driver_version ?? "—" })}</span>
                    {:else}
                      <span class="sys-versions-head">{$t("view.drivers.onThisPc")}</span>
                      <ul class="ver-list">
                        {#each versions as v (v.publishedName)}
                          <li class="ver-row" class:current={v.current}>
                            <span class="ver-num mono">{v.version}</span>
                            {#if v.current}<span class="chip chip-success small-chip">{$t("view.drivers.installed")}</span>{/if}
                            {#if v.date}<span class="ver-date mono">{v.date}</span>{/if}
                            <span class="ver-name mono">{v.publishedName}</span>
                          </li>
                        {/each}
                      </ul>
                      <span class="sys-versions-head">{$t("view.drivers.latestAvailable")}</span>
                      <span class="ver-latest mono">{update.driver_version ?? "—"}{update.driver_date ? ` · ${update.driver_date}` : ""}</span>
                    {/if}
                  </div>
                {/if}
              </li>
            {/each}
          </ul>
        </section>
      {/each}
    </div>
    <DeviceInventory />
  </section>

  {#if hasNvidia}
    <section class="feature-block profile-block" id="drivers-profiles">
      <div class="section-head feature-section-head">
        <span class="section-title">{$t("view.drivers.dlssOverrides")}</span>
        <span class="vendor-pill" data-vendor="nvidia">NVIDIA</span>
      </div>
      <p class="feature-sub">
        {$t("view.drivers.dlssOverridesSub")}
      </p>
      <DlssOverridePanel scope={{ scope: "global" }} driverPacked={nvidiaPacked} />
    </section>
  {/if}

  {#if historyTarget}
    <DriverHistoryFlyout
      vendor={historyTarget.vendor}
      model={historyTarget.model}
      accent={historyTarget.accent}
      onClose={() => (historyTarget = null)}
    />
  {/if}

  {#if nonNvidia.length > 0}
    <section class="feature-block muted-block">
      <div class="section-head feature-section-head">
        <span class="section-title">{$t("view.drivers.upscalingOn", { vendors: `${nonNvidia.includes("amd") ? "AMD" : ""}${nonNvidia.includes("amd") && nonNvidia.includes("intel") ? " / " : ""}${nonNvidia.includes("intel") ? "Intel" : ""}` })}</span>
      </div>
      <p class="feature-sub">
        {$t("view.drivers.upscalingSub")}
      </p>
    </section>
  {/if}
  {/if}
</section>

<style>
  .drivers-view { display: flex; flex-direction: column; gap: 20px; }
  .drivers-view :global(.view-header) { margin-bottom: 0; }
  .check-btn {
    height: 38px;
    padding: 0 16px;
    border-radius: var(--radius-lg);
    background: var(--accent);
    color: var(--accent-fg);
    font-size: 13px;
    font-weight: 600;
    white-space: nowrap;
    transition: background var(--dur-fast) var(--ease);
  }
  .check-btn:hover:not(:disabled) { background: var(--accent-hover); }
  .check-btn:disabled { opacity: 0.6; cursor: default; }
  .error-banner {
    position: relative;
    overflow: hidden;
    padding: 10px 14px 10px 16px;
    border-radius: var(--radius-lg);
    background: var(--danger-dim);
    color: var(--danger);
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }
  .empty { color: var(--text-muted); font-size: 14px; padding: 24px 0; }








  .driver-list { display: flex; flex-direction: column; gap: 10px; list-style: none; padding: 0; margin: 0; }
  .driver-card {
    padding: 14px 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--bg-card);
  }
  .card-row { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  .card-main { display: flex; align-items: center; gap: 14px; min-width: 0; }
  .vendor-pill {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.02em;
    padding: 4px 9px;
    border-radius: var(--radius-full);
    background: var(--bg-elevated);
    color: var(--text-secondary);
    flex-shrink: 0;
  }
  .vendor-pill[data-vendor="nvidia"] { color: var(--vendor-nvidia); }
  .vendor-pill[data-vendor="amd"] { color: var(--vendor-amd); }
  .vendor-pill[data-vendor="intel"] { color: var(--vendor-intel); }
  .model-block { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .model { font-size: 14px; font-weight: 600; color: var(--text-primary); }
  .versions { font-size: 12px; color: var(--text-muted); font-variant-numeric: tabular-nums; display: inline-flex; align-items: center; gap: 6px; }
  .arrow { color: var(--text-muted); }
  .next { color: var(--accent); font-weight: 600; }
  .chan-chip { font-size: 9px; font-weight: 700; padding: 1px 6px; border-radius: var(--radius-full); letter-spacing: 0.04em; }
  .chan-chip.whql { background: var(--success-dim); color: var(--success); }
  .chan-chip.beta { background: var(--warning-dim); color: var(--warning); }
  .card-side { display: flex; align-items: center; gap: 8px; flex-shrink: 0; }
  .driver-update {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 14px;
    border-radius: var(--radius-md);
    background: var(--accent);
    color: var(--accent-fg);
    font-size: 12.5px;
    font-weight: 600;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08);
    transition: background var(--dur-fast) var(--ease);
  }
  .driver-update:hover:not(:disabled) { background: var(--accent-hover); }
  .driver-update:disabled { opacity: 0.55; cursor: default; }
  .driver-update-label { font-variant-numeric: tabular-nums; }
  .driver-update-size {
    font-size: 11px;
    font-weight: 600;
    padding: 1px 6px;
    border-radius: var(--radius-full);
    background: rgba(0, 0, 0, 0.18);
    opacity: 0.9;
    font-variant-numeric: tabular-nums;
  }
  .driver-state {
    font-size: 12px;
    font-weight: 600;
    padding: 5px 12px;
    border-radius: var(--radius-full);
    background: var(--bg-elevated);
    color: var(--text-muted);
  }
  .driver-state[data-tone="success"] { background: var(--success-dim); color: var(--success); }
  .driver-state[data-tone="accent"] { background: var(--update-dim); color: var(--update); }
  .driver-state[data-tone="warning"] { background: var(--warning-dim); color: var(--warning); }
  .driver-state.reboot-pending { display: inline-flex; align-items: center; gap: 6px; }
  .driver-state.reboot-pending svg { flex-shrink: 0; }
  .driver-secondary { display: inline-flex; align-items: center; gap: 4px; }
  .driver-icon {
    width: 34px;
    height: 34px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-md);
    background: var(--bg-elevated);
    color: var(--text-muted);
    border: 1px solid transparent;
    transition:
      color var(--dur-fast) var(--ease),
      background var(--dur-fast) var(--ease),
      border-color var(--dur-fast) var(--ease);
  }
  .driver-icon:hover { color: var(--text-primary); border-color: var(--border-strong); }
  .link-btn { height: 32px; padding: 0 10px; border-radius: var(--radius-lg); background: var(--bg-elevated); color: var(--text-secondary); font-size: 12px; font-weight: 600; }
  .link-btn:hover { color: var(--text-primary); }
  .link-btn.inline { height: 28px; margin-top: 4px; }
  .changelog-toggle {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-top: 12px;
    padding: 4px 2px;
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .changelog-toggle:hover { color: var(--text-primary); }
  .changelog-toggle .chev { transition: transform 0.15s var(--ease); }
  .changelog-toggle .chev.open { transform: rotate(180deg); }
  .changelog {
    margin-top: 8px;
    padding: 12px 14px;
    border-radius: var(--radius-md);
    background: var(--bg-elevated);
    font-size: 12.5px;
    color: var(--text-secondary);
    line-height: 1.5;
  }
  .cl-highlights { margin: 0 0 8px; padding-left: 18px; }
  .cl-highlights li { font-weight: 600; color: var(--text-primary); }
  .cl-label { font-size: 10px; font-weight: 700; text-transform: uppercase; letter-spacing: var(--letter-wider); color: var(--text-muted); }
  .cl-fixed { margin: 4px 0 0; padding-left: 18px; }
  .cl-fixed li { margin-bottom: 2px; }
  .cl-empty { margin: 0; color: var(--text-muted); }

  .feature-block {
    padding: 18px 20px;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--bg-card);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .feature-block.muted-block { background: transparent; }
  .feature-block.system-block { overflow: hidden; }
  .feature-section-head { margin: 0; }
  .feature-sub { font-size: 12.5px; color: var(--text-muted); line-height: 1.55; max-width: 70ch; margin: 0; }
  .beta-tag {
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: var(--letter-wider);
    padding: 2px 7px;
    border-radius: var(--radius-full);
    color: var(--warning);
    background: var(--warning-dim);
    border: 1px solid color-mix(in oklab, var(--warning) 35%, transparent);
  }
  .beta-note {
    font-size: 12px;
    color: var(--warning);
    line-height: 1.5;
    max-width: 70ch;
    margin: 0;
  }
  .install-live { display: flex; flex-direction: column; align-items: flex-end; gap: 4px; min-width: 180px; }
  .install-stage { font-size: 11px; font-weight: 600; text-transform: capitalize; color: var(--accent-progress); }
  .install-bar {
    position: relative;
    width: 180px;
    height: 6px;
    border-radius: var(--radius-full);
    background: var(--bg-elevated);
    overflow: hidden;
    box-shadow: inset 0 0 0 1px var(--border);
  }
  .install-fill {
    height: 100%;
    border-radius: inherit;
    background: linear-gradient(90deg, var(--accent-progress), color-mix(in oklab, var(--accent-progress) 60%, #ffffff));
    box-shadow: 0 0 8px color-mix(in oklab, var(--accent-progress) 55%, transparent);
    transition: none;
  }
  .install-bar.indeterminate .install-fill { width: 40%; animation: installSlide 1.2s var(--ease) infinite; }
  @keyframes installSlide { 0% { transform: translateX(-130%); } 100% { transform: translateX(330%); } }
  .install-live.is-failed .install-stage { color: var(--danger); }
  .install-live.is-failed .install-fill { background: var(--danger); box-shadow: none; animation: none; }
  .install-msg { font-size: 10px; color: var(--text-muted); max-width: 220px; text-align: right; }
  .install-failed-note { font-size: 11px; font-weight: 600; color: var(--danger); text-align: right; }
  .install-live.is-failed .install-msg { color: var(--danger); }

  .driver-update.open-page { background: var(--bg-elevated); color: var(--text-primary); border: 1px solid var(--border-strong); box-shadow: none; }
  .driver-update.open-page:hover:not(:disabled) { background: var(--bg-card); border-color: var(--accent); }
  .ext-arrow { font-size: 12px; opacity: 0.7; }
  .state-block { display: inline-flex; flex-direction: column; align-items: flex-end; gap: 3px; }
  .help-link { background: none; border: none; padding: 0; font-size: 11px; font-weight: 600; color: var(--accent); cursor: pointer; }
  .help-link:hover { text-decoration: underline; }

  .driver-update:focus-visible,
  .driver-icon:focus-visible,
  .help-link:focus-visible,
  .changelog-toggle:focus-visible,
  .check-btn:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }

  @media (prefers-reduced-motion: reduce) {
    .install-fill { transition: none; }
  }

  /* System & Components */
  .check-btn.ghost {
    background: transparent;
    color: var(--text-secondary);
    border: 1px solid var(--border-strong);
    height: 32px;
    padding: 0 12px;
    font-size: 12px;
    margin-left: auto;
  }
  .check-btn.ghost:hover:not(:disabled) { background: var(--bg-elevated); color: var(--text-primary); }

  .sys-groups { display: flex; flex-direction: column; gap: 22px; margin-top: 4px; }
  .sys-group { display: flex; flex-direction: column; gap: 10px; }
  .sys-group-head {
    display: flex;
    align-items: center;
    gap: 9px;
    padding-bottom: 9px;
    border-bottom: 1px solid var(--border);
  }
  .sys-group-icon {
    width: 28px;
    height: 28px;
    border-radius: var(--radius-md);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: var(--accent-dim);
    color: var(--accent);
    flex-shrink: 0;
  }
  .sys-group-label {
    font-size: 13.5px;
    font-weight: 700;
    letter-spacing: var(--letter-tight);
    color: var(--text-primary);
  }
  .sys-group-count {
    font-family: var(--font-mono);
    font-size: var(--fs-2xs);
    font-weight: 700;
    padding: 1px 7px;
    border-radius: var(--radius-full);
    background: var(--bg-elevated);
    color: var(--text-muted);
    margin-left: 2px;
  }
  .sys-list { display: flex; flex-direction: column; gap: 8px; list-style: none; padding: 0; margin: 0; }
  .sys-card {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--bg-elevated);
    transition: border-color var(--dur-fast) var(--ease), background var(--dur-fast) var(--ease);
  }
  .sys-row { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  .sys-card:hover { border-color: var(--border-hover); background: var(--bg-card-hover); }
  .sys-card.is-installing { border-color: var(--accent-ring); }
  .sys-main { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .sys-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    letter-spacing: var(--letter-tight);
  }
  .sys-meta { font-size: 11.5px; color: var(--text-muted); display: inline-flex; align-items: center; gap: 7px; flex-wrap: wrap; }
  .sys-provider { color: var(--text-secondary); font-weight: 500; }
  .sys-versions { display: inline-flex; align-items: center; gap: 6px; font-variant-numeric: tabular-nums; }
  .sys-cur { color: var(--text-muted); }
  .sys-arrow { color: var(--text-placeholder); }
  .sys-new { color: var(--accent); font-weight: 600; }
  .sys-date { color: var(--text-muted); }
  .sys-dot { opacity: 0.45; }
  .sys-side { flex-shrink: 0; display: inline-flex; align-items: center; gap: 8px; }
  .driver-icon.is-on { background: var(--accent-dim); color: var(--accent); border-color: var(--accent-ring); }

  .disclosure { display: flex; flex-direction: column; gap: 6px; }
  .disclosure-summary { margin: 0; }
  .disclosure-toggle {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    align-self: flex-start;
    padding: 4px 2px;
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .disclosure-toggle:hover { color: var(--text-primary); }
  .disclosure-toggle .chev { transition: transform var(--dur-fast) var(--ease); }
  .disclosure-toggle .chev.open { transform: rotate(180deg); }
  .disclosure-toggle:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .disclosure-detail { display: flex; flex-direction: column; gap: 10px; }

  .sys-scanning { display: flex; flex-direction: column; gap: 12px; padding: 8px 0 4px; }
  .sys-scanning-label {
    display: inline-flex;
    align-items: center;
    gap: 9px;
    margin: 0;
    font-size: 13px;
    color: var(--text-secondary);
  }
  .sys-skeletons { display: flex; flex-direction: column; gap: 8px; list-style: none; padding: 0; margin: 0; }
  .sys-skeleton-card {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--bg-elevated);
  }
  .skel-icon { width: 28px; height: 28px; border-radius: var(--radius-md); flex-shrink: 0; }
  .skel-lines { display: flex; flex-direction: column; gap: 6px; flex: 1; min-width: 0; }
  .skel-line { height: 11px; border-radius: var(--radius-full); }
  .skel-line-wide { width: 60%; }
  .skel-line-narrow { width: 34%; }
  .skel-btn { width: 84px; height: 28px; border-radius: var(--radius-md); flex-shrink: 0; }

  .admin-note {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 0;
    padding: 9px 12px;
    border-radius: var(--radius-md);
    background: var(--bg-card);
    border: 1px solid var(--border);
    font-size: 12px;
    line-height: 1.45;
    color: var(--text-muted);
  }
  .admin-note svg { flex-shrink: 0; margin-top: 2px; color: var(--text-secondary); }

  .sys-versions-panel {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px 12px;
    border-radius: var(--radius-md);
    background: var(--bg-card);
    border: 1px solid var(--border);
  }
  .sys-versions-msg { font-size: 12px; color: var(--text-muted); }
  .sys-versions-msg.is-error { color: var(--danger); }
  .sys-versions-head {
    font-size: var(--fs-2xs);
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: var(--letter-wider);
    color: var(--text-muted);
  }
  .ver-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 4px; }
  .ver-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-secondary);
    flex-wrap: wrap;
  }
  .ver-row.current .ver-num { color: var(--accent); font-weight: 700; }
  .ver-num { font-variant-numeric: tabular-nums; }
  .ver-date { color: var(--text-muted); }
  .ver-name { color: var(--text-placeholder); margin-left: auto; }
  .ver-latest { font-size: 12px; color: var(--accent); font-weight: 600; font-variant-numeric: tabular-nums; }
  .small-chip { padding: 1px 7px; font-size: var(--fs-2xs); letter-spacing: 0.04em; }

  .drivers-view .drivers-heading { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: start; gap: 20px; margin-bottom: 8px; }
  .drivers-heading .header-actions { padding-top: 2px; }
  .drivers-tabs { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 0; border-bottom: 1px solid var(--border); margin: 8px 0 12px; }
  .drivers-tabs button { min-height: 48px; padding: 12px 10px; display: flex; align-items: center; justify-content: center; gap: 8px; border-bottom: 2px solid transparent; color: var(--text-secondary); font-size: 13px; line-height: 1.4; }
  .drivers-tabs button.active { border-bottom-color: var(--text-primary); color: var(--text-primary); font-weight: 600; }
  .drivers-tabs button:hover { background: var(--bg-elevated); }
  .drivers-tabs button > span { min-width: 18px; padding: 1px 5px; border-radius: 5px; background: var(--bg-elevated); font-size: 11px; font-variant-numeric: tabular-nums; }
  .driver-list { gap: 20px; }
  .driver-card { padding: 24px; border-radius: 12px; background: var(--bg-card); }
  .driver-card .card-row { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 24px; }
  .driver-card .card-main { flex-direction: column; align-items: flex-start; gap: 20px; }
  .driver-card .vendor-pill { padding: 0; border: 0; background: none; border-radius: 0; font-size: 20px; letter-spacing: 0; }
  .driver-card .vendor-pill :global(.brand-mark) { gap: 10px; }
  .driver-card .model-block { gap: 10px; }
  .driver-card .model { font-size: 21px; line-height: 1.35; font-weight: 600; white-space: normal; overflow-wrap: anywhere; }
  .driver-card .versions { font-size: 14px; display: flex; flex-wrap: wrap; gap: 8px; }
  .driver-card .card-side { display: flex; flex-direction: column; align-items: flex-end; gap: 14px; }
  .driver-card .driver-secondary { display: flex; gap: 8px; }
  .driver-card .driver-icon { width: 36px; height: 36px; border-radius: 8px; }
  .driver-card .changelog-toggle { margin-top: 22px; padding-top: 16px; border-top: 1px solid var(--border); border-radius: 0; width: 100%; justify-content: flex-start; }
  .feature-block { padding: 24px; border-radius: 12px; }
  @container workspace (max-width: 560px) {
    .sys-row { flex-direction: column; align-items: stretch; gap: 16px; }
    .sys-side { justify-content: flex-end; flex-wrap: wrap; }
    .sys-name { white-space: normal; overflow: visible; text-overflow: clip; line-height: 1.5; }
    .sys-versions { flex-wrap: wrap; }
    .feature-block { padding: 20px; }
    .driver-card .card-row { grid-template-columns: minmax(0, 1fr); gap: 24px; }
    .driver-card .card-side { width: 100%; flex-direction: row; align-items: center; justify-content: space-between; flex-wrap: wrap; }
    .drivers-view .drivers-heading { gap: 16px; }
  }
  @container workspace (max-width: 400px) {
    .drivers-view .drivers-heading { grid-template-columns: minmax(0, 1fr); }
    .drivers-tabs { grid-template-columns: minmax(0, 1fr); }
    .drivers-tabs button { justify-content: flex-start; min-height: 42px; }
  }

  .drivers-tabs { position: sticky; top: 0; z-index: 5; background: var(--bg-base); }
  #drivers-graphics, #drivers-system, #drivers-profiles { scroll-margin-top: 80px; }
  .drivers-view { gap: 28px; }
</style>
