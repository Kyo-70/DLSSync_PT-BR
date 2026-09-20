<script lang="ts">
  import { onMount } from "svelte";
  import { Search, X, RefreshCw, ChevronRight, ExternalLink } from "@lucide/svelte";
  import { catalogVendors, bootstrapCatalog, loadCatalog, manifestUpdatedAt, catalogStatus,
    hardwarePreference, type CatalogFamily } from "../lib/stores";
  import { preferredVendor } from "../lib/hardwarePreference";
  import { featureFromFamily, familyLabel, vendorAccent, vendorPortal } from "../lib/labels";
  import CatalogVersionsFlyout from "../components/CatalogVersionsFlyout.svelte";
  import BrandMark from "../components/BrandMark.svelte";
  import { t } from "../lib/i18n/index";
  import { isNexusBuild } from "../lib/distribution";

  let refreshing = $state(false);
  let runtimeQuery = $state("");
  let selectedVendor = $state("");
  let flyoutTarget = $state<{ vendor: string; vendorLabel: string; family: CatalogFamily } | null>(null);
  const vendorOrder = ["nvidia", "amd", "intel", "microsoft"];
  let view = $derived([...$catalogVendors].sort((a, b) => vendorOrder.indexOf(a.vendor) - vendorOrder.indexOf(b.vendor)));
  let totals = $derived({ families: view.reduce((n, v) => n + v.families.length, 0),
    versions: view.reduce((n, v) => n + v.families.reduce((sum, f) => sum + f.releaseCount, 0), 0) });
  let filteredView = $derived.by(() => {
    const query = runtimeQuery.trim().toLowerCase();
    return view.filter(v => !selectedVendor || selectedVendor === v.vendor).map(v => ({ ...v,
      families: !query || v.label.toLowerCase().includes(query) || v.vendor.includes(query)
        ? v.families
        : v.families.filter(f => familyLabel(f.family).toLowerCase().includes(query)
          || f.family.toLowerCase().includes(query) || f.label.toLowerCase().includes(query)),
    })).filter(v => v.families.length > 0);
  });

  onMount(() => {
    void bootstrapCatalog();
  });

  function isSupportFamily(family: string): boolean {
    return family.startsWith("sl_") || family.startsWith("streamline") || family === "xess_sr_dx11" || family === "direct_storage_core" || family === "fsr_loader" || family === "xell";
  }
  let preferredView = $derived(filteredView.filter(vendor => selectedVendor || runtimeQuery.trim() || preferredVendor(vendor.vendor, $hardwarePreference)));
  let otherView = $derived(filteredView.filter(vendor => !selectedVendor && !runtimeQuery.trim() && !preferredVendor(vendor.vendor, $hardwarePreference)));

  async function refresh(): Promise<void> {
    if (refreshing) return;
    refreshing = true;
    try { await loadCatalog({ trigger: "manual_user" }); }
    finally { refreshing = false; }
  }
  async function openExternal(url: string): Promise<void> {
    try { const { open } = await import("@tauri-apps/plugin-shell"); await open(url); }
    catch { window.open(url, "_blank", "noopener,noreferrer"); }
  }
</script>

<div class="catalog-page">
  <header class="view-header">
    <div>
      <h1 class="view-title">{$t("view.catalog.title")}</h1>
      <p class="view-subtitle">{$t("view.catalog.browseSummary", totals)}</p>
    </div>
    <div class="catalog-refresh">
      <button class="btn btn-primary" onclick={refresh} disabled={refreshing}>
        <RefreshCw size={15} class={refreshing ? "refresh-spinning" : ""} aria-hidden="true" />
        {$t(refreshing ? "view.catalog.refreshing" : "view.catalog.refreshManifest")}
      </button>
      <span class="catalog-updated">{$t("view.catalog.stat.updated")} <time>{$manifestUpdatedAt || "—"}</time></span>
    </div>
  </header>

  {#if view.length === 0}
    <div class="catalog-empty" role="status">
      <p>{$t("view.catalog.empty.before")} <strong>{$t("view.catalog.refreshManifest")}</strong> {$t("view.catalog.empty.after")}</p>
    </div>
  {:else}
    <div class="catalog-tools">
      <div class="runtime-search">
        <Search size={17} aria-hidden="true" />
        <input type="search" aria-label={$t("view.catalog.filterPlaceholder")} placeholder={$t("view.catalog.filterPlaceholder")} bind:value={runtimeQuery} />
        {#if runtimeQuery}
          <button class="search-clear" onclick={() => (runtimeQuery = "")} aria-label={$t("view.catalog.clearSearch")}><X size={16} /></button>
        {/if}
      </div>
      <div class="vendor-filters" role="group" aria-label={$t("view.catalog.stat.vendors")}>
        <button class="vendor-filter" aria-pressed={!selectedVendor} onclick={() => (selectedVendor = "")}>{$hardwarePreference.known ? $t("component.hardware.forThisPc") : $t("view.catalog.allVendors")}</button>
        {#each view as v (v.vendor)}
          <button class="vendor-filter" aria-label={v.label} aria-pressed={selectedVendor === v.vendor} onclick={() => (selectedVendor = selectedVendor === v.vendor ? "" : v.vendor)}>
            <BrandMark key={v.vendor} label={v.label} fit="wordmark" size={14} />
          </button>
        {/each}
      </div>
    </div>
    {#if filteredView.length === 0}
      <div class="catalog-empty" role="status">
        <p>{$t("view.catalog.noMatches")}</p>
        <button class="btn btn-ghost" onclick={() => { runtimeQuery = ""; selectedVendor = ""; }}>{$t("view.catalog.clearSearch")}</button>
      </div>
    {/if}
    {#snippet familyRows(v: (typeof view)[number], families: CatalogFamily[])}
          <ul class="catalog-families">
            {#each families as f (f.family)}
              <li>
                <button class="catalog-family" onclick={() => (flyoutTarget = { vendor: v.vendor, vendorLabel: v.label, family: f })} aria-label={$t("view.catalog.viewVersionsAria", { name: familyLabel(f.family) })}>
                  <span class="catalog-family-info">
                    <span class="catalog-family-name">{familyLabel(f.family)}</span>
                  </span>

                  <span class="catalog-family-meta"><span class="catalog-family-version">v{f.latest}</span> <span class="catalog-family-count">{$t("view.catalog.versionsCount", { count: f.releaseCount })}</span></span>
                  <ChevronRight size={16} class="catalog-family-arrow" aria-hidden="true" />
                </button>
              </li>
            {/each}
          </ul>
    {/snippet}
    {#snippet vendorSection(v: (typeof view)[number])}
        {@const portal = vendorPortal(v.vendor)}
        {@const primary = v.families.filter(family => runtimeQuery.trim() || !isSupportFamily(family.family))}
        {@const supporting = runtimeQuery.trim() ? [] : v.families.filter(family => isSupportFamily(family.family))}
        <section class="catalog-vendor" aria-label={v.label}>
          <header class="catalog-vendor-header">
            <h2 class="catalog-vendor-name"><BrandMark key={v.vendor} label={v.label} fit="wordmark" size={24} /></h2>
            <span class="catalog-vendor-count">{$t("view.catalog.versionsCount", { count: v.families.reduce((n, f) => n + f.releaseCount, 0) })}</span>
            {#if portal}
              <button class="catalog-vendor-link" onclick={() => openExternal(portal.url)} title={portal.label} aria-label={$t("view.catalog.openPortal", { name: portal.label })}><ExternalLink size={16} /></button>
            {/if}
          </header>
          {@render familyRows(v, primary)}
          {#if supporting.length > 0}
            <details class="catalog-support"><summary>{$t("component.hardware.supportLibraries")} ({supporting.length})</summary><p>{$t("component.hardware.supportHelp")}</p>{@render familyRows(v, supporting)}</details>
          {/if}

        </section>

    {/snippet}
    <div class="catalog-sections">
      {#each preferredView as v (v.vendor)}{@render vendorSection(v)}{/each}
      {#if otherView.length > 0}
        <details class="catalog-other"><summary>{$t("component.hardware.otherTechnologies")} ({otherView.length})</summary><p>{$t("component.hardware.catalogOtherHelp")}</p>{#each otherView as v (v.vendor)}{@render vendorSection(v)}{/each}</details>
      {/if}
    </div>

    <footer class="catalog-foot">
      <span>{$t("view.catalog.foot.status", { status: $catalogStatus.label })}</span>
      <span>{isNexusBuild ? $t("view.catalog.foot.nexusManual") : $t("view.catalog.foot.autoRefresh")}</span>
    </footer>
  {/if}
</div>

{#if flyoutTarget}
  <CatalogVersionsFlyout vendor={flyoutTarget.vendor} vendorLabel={flyoutTarget.vendorLabel}
    latestVersion={flyoutTarget.family.latest} catalogKey={flyoutTarget.family.family} featureSlot={featureFromFamily(flyoutTarget.family.family)}
    accent={vendorAccent(flyoutTarget.vendor)} families={[flyoutTarget.family.family]}
    onClose={() => (flyoutTarget = null)} />
{/if}

<style>
  .catalog-page { display: flex; flex-direction: column; min-width: 0; }
  .catalog-refresh { display: flex; flex-direction: column; align-items: flex-end; gap: 10px; }
  .catalog-updated { display: flex; flex-wrap: wrap; gap: 6px; font-size: 12px; color: var(--text-muted); }
  .catalog-updated time { font-variant-numeric: tabular-nums; }
  .catalog-tools { display: flex; flex-direction: column; gap: 20px; margin: 4px 0 32px; }
  .runtime-search { display: flex; align-items: center; gap: 12px; width: min(100%, 640px); min-width: 0; padding: 0 14px; min-height: 46px; border: 1px solid var(--border); border-radius: 10px; color: var(--text-muted); background: var(--bg-input); box-sizing: border-box; }
  .runtime-search:focus-within { border-color: var(--text-secondary); }
  .runtime-search input { min-width: 0; width: 100%; padding: 10px 0; border: 0; background: none; box-shadow: none; font-size: 14px; outline: none; }
  .runtime-search input::-webkit-search-cancel-button { display: none; }
  .search-clear { display: grid; place-items: center; width: 28px; height: 28px; flex-shrink: 0; border-radius: 6px; }
  .search-clear:hover { background: var(--bg-elevated); color: var(--text-primary); }
  .vendor-filters { display: flex; flex-wrap: wrap; gap: 8px; }
  .vendor-filter { display: inline-flex; align-items: center; justify-content: center; gap: 8px; padding: 10px 16px; min-height: 42px; border: 1px solid transparent; border-radius: 8px; font-size: 13px; color: var(--text-secondary); }
  .vendor-filter:hover { background: var(--bg-elevated); }
  .vendor-filter[aria-pressed="true"] { background: var(--bg-elevated); border-color: var(--border-hover); color: var(--text-primary); }
  .catalog-sections { display: flex; flex-direction: column; gap: 36px; }
  .catalog-vendor { min-width: 0; }
  .catalog-vendor-header { display: flex; align-items: center; gap: 16px; margin-bottom: 14px; min-height: 44px; }
  .catalog-vendor-name { display: flex; align-items: center; font-size: 23px; line-height: 1; letter-spacing: var(--letter-tight); margin: 0; }
  .catalog-vendor-name :global(.brand-mark) { gap: 10px; }
  .catalog-vendor-count { margin-left: auto; color: var(--text-muted); font-size: 13px; white-space: nowrap; }
  .catalog-vendor-link { display: grid; place-items: center; width: 32px; height: 32px; border-radius: 7px; color: var(--text-muted); flex-shrink: 0; }
  .catalog-vendor-link:hover { background: var(--bg-elevated); color: var(--text-primary); }
  .catalog-families { display: grid; grid-template-columns: minmax(0, 1fr); gap: 0; list-style: none; padding: 0; margin: 0; }
  .catalog-families li { min-width: 0; border-top: 1px solid var(--border); }
  .catalog-family { display: grid; grid-template-columns: minmax(0, 1fr) auto 16px; align-items: center; gap: 16px; text-align: left; padding: 16px 12px; width: 100%; min-height: 64px; border-radius: 8px; color: var(--text-primary); transition: background var(--dur-fast) var(--ease); }
  .catalog-family:hover { background: var(--bg-elevated); }
  .catalog-family-info { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
  .catalog-family-name { font-size: 15px; line-height: 1.45; font-weight: 600; overflow-wrap: anywhere; }
  .catalog-family-count { font-size: 12px; color: var(--text-muted); }
  .catalog-family-version { font-family: var(--font-mono); font-size: 12px; color: var(--text-secondary); font-variant-numeric: tabular-nums; overflow-wrap: anywhere; max-width: 15ch; }
  .catalog-family :global(.catalog-family-arrow) { color: var(--text-muted); }
  .catalog-empty { padding: 40px 0; display: flex; flex-direction: column; align-items: flex-start; gap: 16px; color: var(--text-secondary); }
  .catalog-foot { display: flex; justify-content: space-between; flex-wrap: wrap; gap: 12px; padding: 20px 0; font-size: 12px; color: var(--text-muted); border-top: 1px solid var(--border); }
  @container workspace (max-width: 760px) {
    .catalog-families { grid-template-columns: minmax(0, 1fr); }
    .catalog-refresh { align-items: flex-start; }
  }
  @media (prefers-reduced-motion: no-preference) {
    :global(.refresh-spinning) { animation: catalog-spin 1s linear infinite; }
  }
  @keyframes catalog-spin { to { transform: rotate(360deg); } }






















  .catalog-family-count { line-height: 1.5; }
  .catalog-family-version { font-size: 12px; }
  .catalog-support { margin-top: 18px; }
  .catalog-support > summary, .catalog-other > summary { font-size: 14px; color: var(--text-secondary); cursor: pointer; padding: 10px 0; }
  .catalog-support > p, .catalog-other > p { font-size: 13px; line-height: 1.5; color: var(--text-muted); margin: 4px 0 16px; }
  .catalog-support .catalog-family { min-height: 60px; }
  .catalog-other { border-top: 1px solid var(--border); padding-top: 16px; }
  .catalog-other .catalog-vendor { margin-top: 24px; }
  .catalog-family-meta { display: grid; grid-template-columns: minmax(14ch, auto) 100px; align-items: center; gap: 24px; text-align: right; }
  .catalog-family-count { white-space: nowrap; }
  .catalog-family-version { max-width: none; white-space: nowrap; }
  .catalog-vendor-header { margin: 0; padding: 16px 0; }
  .catalog-support, .catalog-other { margin-top: 16px; }
  .catalog-support > summary, .catalog-other > summary { padding: 16px 0; font-weight: 600; }
  .catalog-support > p, .catalog-other > p { max-width: 72ch; }
  @container workspace (max-width: 620px) {
    .catalog-family { grid-template-columns: minmax(0, 1fr) 16px; gap: 8px 16px; }
    .catalog-family-meta { grid-column: 1; grid-row: 2; display: flex; gap: 18px; text-align: left; flex-wrap: wrap; }
    .catalog-family :global(.catalog-family-arrow) { grid-column: 2; grid-row: 1 / span 2; }
  }
</style>
