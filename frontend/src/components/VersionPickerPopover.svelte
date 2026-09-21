<script lang="ts">
  import { onMount } from "svelte";
  import { t } from "../lib/i18n/index";
  import { listReleases, type Release, type DllFamily } from "../lib/api";
  import { familyVendor, familyCatalogKey, familyLabel } from "../lib/labels";
  import { settings } from "../lib/stores";
  import { compareVersions } from "../lib/versions";
  import { sameSha256 } from "../lib/catalogInstallation";
  import FlyoutShell from "./FlyoutShell.svelte";

  let { family, filename, currentVersion, currentSha256 = null, latestVersion, pickedVersion, onPick, onClose }: {
    family: DllFamily;
    filename: string;
    currentVersion: string | null;
    currentSha256?: string | null;
    latestVersion: string | null;
    pickedVersion: string | null;
    onPick: (version: string | null) => void;
    onClose: () => void;
  } = $props();

  let releases = $state<Release[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let query = $state("");
  let stableOnly = $state($settings?.advanced.prefer_stable_channel ?? true);
  let installed = $derived(releases.find(r => sameSha256(r.sha256, currentSha256)));
  let filtered = $derived(releases.filter(r => (!stableOnly || r.channel === "stable") &&
    `${r.version} ${r.release_notes ?? ""}`.toLowerCase().includes(query.trim().toLowerCase())));

  async function load(): Promise<void> {
    loading = true;
    error = null;
    try {
      releases = (await listReleases(familyVendor(family), familyCatalogKey(family)))
        .sort((a, b) => compareVersions(b.version, a.version));
    } catch (err) {
      error = err && typeof err === "object" && "message" in err ? String(err.message) : String(err);
    } finally { loading = false; }
  }
  onMount(() => { void load(); });
  function pick(version: string | null): void { onPick(version); onClose(); }
</script>

<FlyoutShell {onClose} ariaLabel={$t("component.flyout.pickVersion")} width="720px">
  <header class="picker-head">
    <h2>{familyLabel(family)}</h2>
    <p class="mono">{filename}</p>
    <button class="dialog-close" onclick={onClose} aria-label={$t("common.close")}>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m6 6 12 12M6 18 18 6"/></svg>
    </button>
  </header>
  <div class="version-context">
    <section>
      <h3>{$t("component.detail.installedFile")}</h3>
      <strong class="mono">{currentVersion ? `v${currentVersion}` : $t("status.unknown")}</strong>
      {#if installed}<p>{$t("component.detail.catalogVersion")} <span class="mono">v{installed.version}</span></p>
      {:else if !loading}<p>{$t("component.detail.noExactMatch")}</p>{/if}
    </section>
    {#if latestVersion}
      <section>
        <h3>{$t("component.flyout.latest")}</h3>
        <strong class="mono">v{latestVersion}</strong>
        <button class="btn btn-sm btn-accent" onclick={() => pick(null)}>{$t("component.flyout.useLatest")}</button>
      </section>
    {/if}
  </div>
  <div class="picker-toolbar">
    <input type="search" bind:value={query} placeholder={$t("component.flyout.filterVersionsOrNotes")} aria-label={$t("component.flyout.filterVersionsOrNotes")} />
    <label><input type="checkbox" bind:checked={stableOnly} /> {$t("component.flyout.stableOnly")}</label>
  </div>
  <div class="picker-body">
    {#if loading}<p class="picker-state" role="status">{$t("component.flyout.loadingReleaseHistory")}</p>
    {:else if error}<p class="picker-state danger" role="alert">{$t("component.flyout.failedToLoad", { error })}</p><button class="btn btn-ghost" onclick={load}>{$t("component.gameDrawer.scanError.retry")}</button>
    {:else if filtered.length === 0}<p class="picker-state">{$t("component.flyout.noMatches")}</p>
    {:else}
      <ul aria-label={$t("component.detail.availableVersions")}>
        {#each filtered as r (r.version + r.sha256)}
          <li>
            <button class="release-row" class:selected={pickedVersion === r.version} onclick={() => pick(r.version)}>
              <span class="release-main"><strong class="mono">v{r.version}</strong><span>{r.released_at.slice(0, 10)}{r.release_notes ? ` · ${r.release_notes}` : ""}</span></span>
              <span class="release-labels">
                {#if r.version === latestVersion}<span class="latest">{$t("component.flyout.latest")}</span>{/if}
                {#if sameSha256(r.sha256, currentSha256)}<span class="installed">{$t("component.flyout.installed")}</span>{/if}
                {#if r.channel === "experimental"}<span>{$t("component.flyout.beta")}</span>{/if}
              </span>
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m9 5 7 7-7 7"/></svg>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
  <footer>{$t("component.flyout.versionCount", { shown: filtered.length, count: releases.length })}</footer>
</FlyoutShell>

<style>
  .picker-head { padding: 24px 60px 20px 24px; border-bottom: 1px solid var(--border); }
  .picker-head h2 { font-size: 20px; line-height: 1.3; }
  .picker-head p { font-size: 12px; color: var(--text-secondary); margin-top: 8px; overflow-wrap: anywhere; }
  .version-context { display: grid; grid-template-columns: 1fr 1fr; gap: 24px; padding: 20px 24px; border-bottom: 1px solid var(--border); }
  .version-context section { display: flex; flex-direction: column; align-items: flex-start; gap: 8px; min-width: 0; }
  .version-context h3 { font-size: 13px; color: var(--text-secondary); font-weight: 500; }
  .version-context strong { font-size: 18px; overflow-wrap: anywhere; }
  .version-context p { font-size: 12px; color: var(--text-secondary); line-height: 1.5; }
  .picker-toolbar { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; padding: 12px 24px; border-bottom: 1px solid var(--border); }
  .picker-toolbar > input { flex: 1; min-width: 160px; }
  .picker-toolbar label { display: inline-flex; gap: 8px; align-items: center; font-size: 12px; }
  .picker-body { min-height: 0; overflow-y: auto; overscroll-behavior: contain; }
  ul { list-style: none; margin: 0; padding: 0; }
  .release-row { width: 100%; display: grid; grid-template-columns: minmax(0, 1fr) auto 16px; align-items: center; gap: 16px; padding: 18px 24px; border: none; border-bottom: 1px solid var(--border); color: var(--text-primary); text-align: left; background: none; cursor: pointer; }
  .release-row:hover, .release-row.selected { background: var(--bg-elevated); }
  .release-main { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .release-main strong { font-size: 14px; }
  .release-main > span { font-size: 12px; color: var(--text-secondary); overflow-wrap: anywhere; line-height: 1.5; }
  .release-labels { display: flex; flex-direction: column; gap: 6px; font-size: 12px; }
  .latest { color: var(--text-primary); font-weight: 600; }
  .installed { color: var(--success); }
  .picker-state { padding: 32px 24px; color: var(--text-secondary); }
  .danger { color: var(--danger); }
  footer { padding: 12px 24px; border-top: 1px solid var(--border); font-size: 12px; color: var(--text-secondary); }
  @media (max-width: 480px) {
    .version-context { gap: 16px; padding: 16px; }
    .version-context strong { font-size: 15px; }
    .picker-toolbar, .release-row { padding: 14px 16px; }
  }
</style>
