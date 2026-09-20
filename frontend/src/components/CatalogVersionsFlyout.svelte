<script lang="ts">
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { t, locale, translate } from "../lib/i18n/index";
  import { listReleases, type Release } from "../lib/api";
  import {
    familyLabel,
    familyShort,
    type FeatureSlot,
  } from "../lib/labels";
  import type { CatalogFamily } from "../lib/stores";
  import { authoritativeState } from "../lib/stateSync";
  import { installedGameNames, observedFamilyVersions } from "../lib/catalogInstallation";
  import { showToast } from "../lib/stores";
  import { compareVersions } from "../lib/versions";
  import BrandMark from "./BrandMark.svelte";
  import Checkbox from "./Checkbox.svelte";
  import FlyoutShell from "./FlyoutShell.svelte";

  let {
    vendor,
    catalogKey,
    latestVersion = null,
    featureSlot,
    accent,
    advancedFamilies,
    vendorLabel,
    onClose,
  }: {
    vendor: string;
    catalogKey: string;
    latestVersion?: string | null;
    featureSlot: FeatureSlot;
    accent: string;
    families?: string[];
    advancedFamilies?: CatalogFamily[];
    vendorLabel?: string;
    onClose: () => void;
  } = $props();

  type ViewMode = "modules" | "versions";

  let mode = $state<ViewMode>("versions");
  let activeFamily = $state("");
  let activeLatest = $state<string | null>(null);
  let activeFamilyLabel = $state<string>("");

  let releases = $state<Release[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let query = $state("");
  let stableOnly = $state(true);

  let modulesQuery = $state("");
  let installedFiles = $derived(observedFamilyVersions($authoritativeState.games, activeFamily));

  let headerTitle = $derived.by(() => {
    if (mode === "modules") return `${vendorLabel ?? vendor} · ${$t("feature.advanced.title")}`;
    return activeFamilyLabel;
  });
  let headerSubtitle = $derived.by(() => {
    if (mode === "modules") return $t("component.flyout.modulesSubtitle");
    if (featureSlot === "advanced") return $t("component.flyout.advancedTech", { vendor: vendorLabel ?? vendor });
    return $t("feature." + featureSlot + ".blurb");
  });

  onMount(() => {
    if (catalogKey === "advanced") {
      mode = "modules";
      activeFamilyLabel = "";
    } else {
      mode = "versions";
      activeFamilyLabel = familyLabel(catalogKey);
      void loadFamilyVersions(catalogKey);
    }
  });

  function toError(err: unknown): string {
    return err && typeof err === "object" && "message" in err
      ? String((err as { message: unknown }).message)
      : String(err);
  }

  async function loadFamilyVersions(family: string): Promise<void> {
    activeFamily = family;
    activeLatest = family === catalogKey ? latestVersion : advancedFamilies?.find(item => item.family === family)?.latest ?? null;
    loading = true;
    error = null;
    releases = [];
    try {
      const list = await listReleases(vendor, family);
      list.sort((a, b) => compareVersions(b.version, a.version));
      releases = list;
    } catch (err: unknown) {
      error = toError(err);
    } finally {
      loading = false;
    }
  }

  function drillIntoFamily(f: CatalogFamily): void {
    activeFamilyLabel = familyLabel(f.family);
    mode = "versions";
    void loadFamilyVersions(f.family);
  }

  function backToModules(): void {
    if (catalogKey !== "advanced") return;
    mode = "modules";
    releases = [];
    error = null;
  }

  let filtered = $derived(
    releases.filter((r) => {
      if (stableOnly && r.channel !== "stable") return false;
      if (!query) return true;
      const q = query.toLowerCase();
      return (
        r.version.toLowerCase().includes(q) ||
        r.filename.toLowerCase().includes(q) ||
        (r.release_notes ?? "").toLowerCase().includes(q)
      );
    }),
  );
  let hiddenByStable = $derived(
    stableOnly ? releases.filter((r) => r.channel !== "stable").length : 0,
  );

  let filteredModules = $derived.by<CatalogFamily[]>(() => {
    const list = advancedFamilies ?? [];
    if (!modulesQuery.trim()) return list;
    const q = modulesQuery.toLowerCase();
    return list.filter((f) =>
      familyLabel(f.family).toLowerCase().includes(q) ||
      f.family.toLowerCase().includes(q),
    );
  });

  function formatDate(iso: string): string {
    if (!iso) return "—";
    const d = new Date(iso);
    if (isNaN(d.getTime())) return "—";
    return d.toISOString().slice(0, 10);
  }
  function formatSize(bytes: number): string {
    if (!bytes || bytes <= 0) return "—";
    const mb = bytes / (1024 * 1024);
    if (mb >= 1) return `${mb.toFixed(1)} MB`;
    return `${(bytes / 1024).toFixed(0)} KB`;
  }

  async function downloadRelease(r: Release): Promise<void> {
    const loc = get(locale);
    if (!r.cdn_url) {
      showToast("warning", translate(loc, "component.flyout.toast.noDownloadUrl"));
      return;
    }
    try {
      const { open } = await import("@tauri-apps/plugin-shell");
      await open(r.cdn_url);
      showToast(
        "success",
        translate(loc, "component.flyout.toast.opening", { file: r.filename, version: r.version }),
      );
    } catch (err: unknown) {
      showToast("danger", translate(loc, "component.flyout.toast.openFailed", { error: String(err) }));
    }
  }

  async function copyUrl(r: Release): Promise<void> {
    const loc = get(locale);
    if (!r.cdn_url) {
      showToast("warning", translate(loc, "component.flyout.toast.noUrl"));
      return;
    }
    try {
      const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
      await writeText(r.cdn_url);
      showToast(
        "success",
        translate(loc, "component.flyout.toast.copiedUrl", { version: r.version }),
      );
    } catch (err: unknown) {
      showToast("danger", translate(loc, "component.flyout.toast.copyFailed", { error: String(err) }));
    }
  }

  function onEscape(e: KeyboardEvent): void {
    e.stopPropagation();
    if (mode === "versions" && catalogKey === "advanced") {
      backToModules();
    } else {
      onClose();
    }
  }

  function moduleKey(e: KeyboardEvent, f: CatalogFamily): void {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      drillIntoFamily(f);
    }
  }
</script>

<FlyoutShell
  {onClose}
  {accent}
  {onEscape}
  ariaLabel={headerTitle}
  backdropOpacity={0.45}
  backdropBlur={3}
>
  <header class="flyout-head">
    {#if mode === "versions" && catalogKey === "advanced"}
      <button class="flyout-back" onclick={backToModules} aria-label={$t("component.flyout.backToModules")}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
      </button>
    {/if}
    <div class="flyout-glyph" style:color={accent} aria-hidden="true">
      <BrandMark key={vendor} fit="wordmark" size={20} showLabel={false} />
    </div>
    <div class="flyout-title">
      <span class="title-line">{headerTitle}</span>
      <span class="subtitle-line">{headerSubtitle}</span>
    </div>
    <button class="dialog-close" onclick={onClose} aria-label={$t("common.close")}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
    </button>
  </header>

  {#if mode === "versions" && installedFiles.length > 0}
    <details class="installed-files" open>
      <summary>{$t("component.detail.installedFile")}</summary>
      <ul>{#each installedFiles as file (file.version)}
        <li><strong class="mono">v{file.version}</strong><span>{file.games.join(", ")}</span></li>
      {/each}</ul>
    </details>
  {/if}

  {#if mode === "modules"}
    <div class="flyout-toolbar">
      <input
        type="search"
        placeholder={$t("component.flyout.filterModules")}
        bind:value={modulesQuery}
        class="flyout-search"
      />
      <span class="toolbar-count">{$t("component.flyout.moduleCount", { shown: filteredModules.length, count: (advancedFamilies ?? []).length })}</span>
    </div>
    <div class="flyout-body">
      {#if filteredModules.length === 0}
        <div class="flyout-state">
          <p>{$t("component.flyout.noModulesMatch")}</p>
        </div>
      {:else}
        <ul class="module-list">
          {#each filteredModules as f (f.family)}
            <li class="module-row">
              <button
                type="button"
                class="module-row-btn"
                onclick={() => drillIntoFamily(f)}
                onkeydown={(e) => moduleKey(e, f)}
                aria-label={$t("component.flyout.viewVersionsOf", { name: familyLabel(f.family) })}
              >
                <span class="module-glyph" style:color={accent} aria-hidden="true">
                  <BrandMark key={vendor} fit="wordmark" size={12} showLabel={false} />
                </span>
                <div class="module-meta">
                  <span class="module-title">{familyLabel(f.family)}</span>
                  <span class="module-sub">{familyShort(f.family)} · {$t("view.catalog.versionsCount", { count: f.releaseCount })}</span>
                </div>
                <span class="module-latest mono" style:color={accent}>v{f.latest}</span>
                <svg class="module-arrow" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {:else}
    <div class="flyout-toolbar">
      <input
        type="search"
        placeholder={$t("component.flyout.filterVersionsFilesNotes")}
        bind:value={query}
        class="flyout-search"
      />
      <Checkbox
        bind:checked={stableOnly}
        label={hiddenByStable > 0
          ? $t("component.flyout.stableOnlyHidden", { count: hiddenByStable })
          : $t("component.flyout.stableOnly")}
      />
    </div>
    <div class="flyout-body">
      {#if loading}
        <div class="flyout-state">
          <span class="spinner"></span>
          <span>{$t("component.flyout.loadingVersions")}</span>
        </div>
      {:else if error}
        <div class="flyout-state danger">{$t("component.flyout.failedToLoad", { error })}</div>
      {:else if releases.length === 0}
        <div class="flyout-state">
          <p><strong>{$t("component.flyout.noVersionsTracked")}</strong></p>
          <p class="small">{$t("component.flyout.noVersionsTrackedDetail")}</p>
        </div>
      {:else if filtered.length === 0}
        <div class="flyout-state">
          <p>{$t("component.flyout.noMatches")}</p>
          <p class="small">{$t("component.flyout.noMatchesDetail")}</p>
        </div>
      {:else}
        <ul class="version-list">
          {#each filtered as r (r.version + r.sha256)}
            {@const installedIn = installedGameNames($authoritativeState.games, activeFamily, r.sha256)}
            <li class="version-row" class:is-first={r.version === activeLatest}>
              <div class="row-left">
                {#if r.version === activeLatest}<span class="latest-tag">{$t("component.flyout.latest")}</span>{/if}
                <span class="row-version mono">v{r.version}</span>
                {#if installedIn.length}<span class="installed-tag" title={installedIn.join(", ")}>{$t("component.flyout.installed")}</span>{/if}
              </div>
              <div class="row-mid">
                <span class="row-file mono">{r.filename}</span>
                {#if installedIn.length}<span class="installed-context">{$t("component.detail.installedIn", { games: installedIn.join(", ") })}</span>{/if}
                <span class="row-meta">
                  <span class="row-date">{formatDate(r.released_at)}</span>
                  <span class="row-sep">·</span>
                  <span class="row-size">{formatSize(r.size_bytes)}</span>
                  {#if r.channel === "experimental"}
                    <span class="row-sep">·</span>
                    <span class="chip chip-warning small-chip">{$t("component.flyout.beta")}</span>
                  {/if}
                  {#if r.signed}
                    <span class="row-shield" title={r.signature_subject ?? $t("component.flyout.signedByVendor")}>
                      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><polyline points="9 12 11 14 15 10"/></svg>
                    </span>
                  {/if}
                </span>
                {#if r.release_notes}
                  <span class="row-notes truncate" title={r.release_notes}>{r.release_notes}</span>
                {/if}
              </div>
              <div class="row-actions">
                <button class="btn btn-sm btn-accent" onclick={() => downloadRelease(r)} title={$t("component.flyout.openDownloadUrlTitle")}>
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                  {$t("common.download")}
                </button>
                <button class="btn btn-sm btn-ghost" onclick={() => copyUrl(r)} title={$t("component.flyout.copyCdnUrlTitle")}>
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
                </button>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
    <footer class="flyout-foot">
      <p class="installation-note">{$t("component.detail.catalogMatchHelp")}</p>
      <span class="foot-count">{$t("component.flyout.versionCount", { shown: filtered.length, count: releases.length })}</span>
    </footer>
  {/if}
</FlyoutShell>

<style>
  .installed-files { padding: 14px 20px; border-bottom: 1px solid var(--border); font-size: 12px; max-height: 150px; overflow-y: auto; flex-shrink: 0; }
  .installed-files summary { cursor: pointer; font-weight: 600; color: var(--text-primary); }
  .installed-files ul { list-style: none; padding: 8px 0 0; margin: 0; display: flex; flex-direction: column; gap: 8px; }
  .installed-files li { display: flex; flex-wrap: wrap; gap: 6px 16px; color: var(--text-secondary); }
  .installed-files strong { font-weight: 500; color: var(--text-primary); }
  .flyout-head {
    padding: 18px 52px 14px 20px;
    border-bottom: 1px solid var(--border);
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 14px;
    align-items: center;
  }
  .flyout-head:has(.flyout-back) { grid-template-columns: 30px auto minmax(0, 1fr); }
  .flyout-head:not(:has(.flyout-back)) { grid-template-columns: auto minmax(0, 1fr); }
  .flyout-back {
    width: 28px;
    height: 28px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    border-radius: var(--radius-sm);
  }
  .flyout-back:hover { background: var(--bg-elevated); color: var(--text-primary); }
  .flyout-glyph {
    min-width: 44px;
    padding: 0 12px;
    flex-shrink: 0;
    height: 44px;
    border-radius: var(--radius-md);
    background: var(--bg-elevated);
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .flyout-title { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .title-line { font-size: var(--fs-lg); font-weight: 700; color: var(--text-primary); letter-spacing: var(--letter-tight); }
  .subtitle-line { font-size: var(--fs-sm); color: var(--text-secondary); line-height: 1.4; }

  .flyout-toolbar {
    padding: 10px 20px;
    display: flex;
    align-items: center;
    gap: 12px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-input);
  }
  .flyout-search { flex: 1; min-width: 160px; font-size: var(--fs-sm); padding: 7px 12px; }
  .toolbar-count { font-size: var(--fs-xs); color: var(--text-muted); font-variant-numeric: tabular-nums; }

  .flyout-body { flex: 1; min-height: 0; overflow-y: auto; overscroll-behavior: contain; }
  .flyout-state {
    padding: 50px 20px;
    text-align: center;
    color: var(--text-muted);
    font-size: var(--fs-sm);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }
  .flyout-state.danger { color: var(--danger); }
  .flyout-state .small { font-size: var(--fs-xs); opacity: 0.85; max-width: 360px; }

  .module-list { list-style: none; padding: 8px 0; margin: 0; }
  .module-row { padding: 0; }
  .module-row-btn {
    width: 100%;
    display: grid;
    grid-template-columns: 28px 1fr auto 14px;
    align-items: center;
    gap: 12px;
    padding: 12px 20px;
    background: none;
    border: none;
    border-left: 2px solid transparent;
    text-align: left;
    color: inherit;
    cursor: pointer;
    transition: background 0.12s var(--ease), border-color 0.12s var(--ease);
  }
  .module-row-btn:hover { background: var(--bg-elevated); border-left-color: var(--accent); }
  .module-glyph { display: inline-flex; min-width: 52px; }
  .module-meta { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .module-title { font-size: var(--fs-md); color: var(--text-primary); font-weight: 600; letter-spacing: var(--letter-tight); }
  .module-sub { font-size: var(--fs-xs); color: var(--text-muted); text-transform: uppercase; letter-spacing: var(--letter-wider); font-weight: 600; }
  .module-latest { font-size: var(--fs-sm); font-variant-numeric: tabular-nums; font-weight: 700; }
  .module-arrow { color: var(--text-muted); opacity: 0.5; transition: transform 0.12s var(--ease), opacity 0.12s var(--ease); }
  .module-row-btn:hover .module-arrow { opacity: 1; transform: translateX(2px); color: var(--accent); }

  .version-list { list-style: none; padding: 6px 0 10px; margin: 0; }
  .version-row {
    display: grid;
    grid-template-columns: 130px minmax(0, 1fr) auto;
    align-items: center;
    gap: 14px;
    padding: 10px 20px;
    border-bottom: 1px solid var(--border);
  }
  .version-row.is-first { background: var(--accent-soft); }
  .version-row:last-child { border-bottom: none; }

  .row-left { display: flex; flex-direction: column; gap: 2px; }
  .latest-tag {
    font-size: var(--fs-2xs);
    font-weight: 700;
    color: var(--accent);
    letter-spacing: var(--letter-wider);
    text-transform: uppercase;
  }
  .row-version { font-size: var(--fs-md); font-weight: 700; color: var(--text-primary); font-variant-numeric: tabular-nums; }

  .row-mid { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
  .row-file { font-size: var(--fs-xs); color: var(--text-muted); }
  .row-meta { display: inline-flex; align-items: center; gap: 6px; font-size: var(--fs-2xs); color: var(--text-muted); }
  .row-sep { opacity: 0.5; }
  .row-date, .row-size { font-variant-numeric: tabular-nums; }
  .row-shield { color: var(--success); display: inline-flex; }
  .row-notes { font-size: var(--fs-xs); color: var(--text-muted); opacity: 0.85; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .small-chip { padding: 1px 6px; font-size: var(--fs-2xs); letter-spacing: 0.04em; }
  .row-actions { display: inline-flex; gap: 6px; flex-shrink: 0; }

  .flyout-foot {
    padding: 10px 20px;
    border-top: 1px solid var(--border);
    background: var(--bg-input);
  }
  .foot-count { font-size: var(--fs-xs); color: var(--text-muted); font-variant-numeric: tabular-nums; }

  .spinner {
    width: 14px;
    height: 14px;
    border: 2px solid currentColor;
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  .installed-tag { color: var(--success); font-size: 12px; font-weight: 600; }
  .installed-context { color: var(--text-secondary); font-size: 12px; line-height: 1.5; }
  .installation-note { font-size: 12px; color: var(--text-secondary); margin-bottom: 6px; line-height: 1.5; }
  @media (max-width: 640px) {
    .version-row { grid-template-columns: minmax(0, 1fr) auto; gap: 8px; padding: 16px; }
    .row-left { grid-column: 1; }
    .row-mid { grid-column: 1; }
    .row-actions { grid-column: 2; grid-row: 1 / span 2; flex-direction: column; }
    .row-file { overflow-wrap: anywhere; }
    .row-meta { flex-wrap: wrap; }
    .flyout-toolbar { flex-wrap: wrap; gap: 8px; }
    .flyout-search { min-width: 0; flex-basis: 100%; }
  }
</style>
