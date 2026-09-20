<script lang="ts">
  import { onMount } from "svelte";
  import { getSystemDevices, type SystemDevice } from "../lib/api";
  import BrandMark from "./BrandMark.svelte";
  import { resolveBrandKey } from "../lib/brands";
  import { Cpu, ChevronDown } from "@lucide/svelte";
  import { t } from "../lib/i18n/index";

  let devices = $state<SystemDevice[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let query = $state("");
  let problemsOnly = $state(false);
  let problemCount = $derived(devices.filter(device => (device.problem_code ?? 0) !== 0).length);
  let groups = $derived.by(() => {
    const result = new Map<string, SystemDevice[]>();
    const term = query.trim().toLowerCase();
    for (const device of devices) {
      if (problemsOnly && !device.problem_code) continue;
      if (term && !`${device.name} ${device.manufacturer} ${device.class} ${device.hardware_id} ${device.inf_name ?? ""}`.toLowerCase().includes(term)) continue;
      const rows = result.get(device.class) ?? [];
      rows.push(device);
      result.set(device.class, rows);
    }
    return [...result.entries()].sort(([a], [b]) => a.localeCompare(b));
  });
  async function refresh(): Promise<void> {
    if (loading) return;
    loading = true;
    error = null;
    try { devices = (await getSystemDevices()) ?? []; }
    catch (err) { error = err && typeof err === "object" && "message" in err ? String(err.message) : String(err); }
    finally { loading = false; }
  }
  onMount(() => { void refresh(); });
</script>

<section class="device-inventory" aria-label={$t("component.devices.heading")}>
  <header>
    <div><h3>{$t("component.devices.heading")} <span>{devices.length}</span></h3><p>{$t("component.devices.localHelp")}</p></div>
    <button class="btn btn-sm btn-ghost" onclick={refresh} disabled={loading}>{$t("view.drivers.rescan")}</button>
  </header>
  <div class="device-tools">
    <input type="search" placeholder={$t("component.devices.search")} aria-label={$t("component.devices.search")} bind:value={query} />
    <label><input type="checkbox" bind:checked={problemsOnly} /> {$t("component.devices.attention")} <span>{problemCount}</span></label>
  </div>
  {#if error}<p class="device-error" role="alert">{error}</p>
  {:else if loading && !devices.length}<p role="status">{$t("view.drivers.scanning")}</p>
  {:else if !groups.length}<p class="device-empty">{$t("component.devices.noMatches")}</p>{/if}
  {#each groups as [deviceClass, rows] (deviceClass)}
    <details class="device-class" open={!!query.trim() || problemsOnly}>
      <summary><span>{$t("component.devices.classes." + deviceClass)}</span><span class="count">{rows.length}</span><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m9 5 7 7-7 7"/></svg></summary>
      <ul>
        {#each rows as device (device.hardware_id)}
          <li>
            <span class="device-logo" aria-hidden="true">{#if resolveBrandKey(device.manufacturer)}<BrandMark key={device.manufacturer} size={22} showLabel={false} />{:else}<Cpu size={20} />{/if}</span>
            <div class="device-name"><strong>{device.name}</strong><span>{device.manufacturer}</span></div>
            <div class="device-version" aria-label={$t("component.flyout.installed")}><strong class="mono">{device.driver_version ?? $t("status.unknown")}</strong></div>
            <details class="device-info"><summary>{$t("component.devices.details")}<ChevronDown size={14} aria-hidden="true" /></summary><dl>
              <dt>{$t("component.devices.identity")}</dt><dd class="mono">{device.hardware_id}</dd>
              {#if device.inf_name}<dt>INF</dt><dd class="mono">{device.inf_name}</dd>{/if}
              {#if device.driver_date}<dt>{$t("component.devices.date")}</dt><dd>{device.driver_date}</dd>{/if}
            </dl></details>
            {#if device.problem_code}<p class="device-problem">{$t("component.devices.problem", { code: device.problem_code })}</p>{/if}
          </li>
        {/each}
      </ul>
    </details>
  {/each}
</section>

<style>
  .device-inventory { margin-top: 32px; padding-top: 24px; border-top: 1px solid var(--border); }
  header { display: flex; align-items: flex-start; justify-content: space-between; gap: 20px; }
  h3 { font-size: 18px; line-height: 1.4; }
  h3 span { font-size: 13px; font-weight: 400; color: var(--text-secondary); margin-left: 8px; }
  header p { color: var(--text-secondary); font-size: 13px; line-height: 1.6; margin-top: 6px; max-width: 70ch; }
  .device-tools { display: flex; flex-wrap: wrap; align-items: center; gap: 16px; margin: 20px 0; }
  .device-tools > input { flex: 1; min-width: 180px; }
  .device-tools label { display: inline-flex; gap: 8px; align-items: center; font-size: 13px; }
  .device-class { border-bottom: 1px solid var(--border); }
  .device-class > summary { padding: 18px 0; display: flex; align-items: center; gap: 12px; list-style: none; font-weight: 600; cursor: pointer; }
  .device-class > summary::-webkit-details-marker { display: none; }
  .count { color: var(--text-secondary); font-size: 12px; font-weight: 400; }
  .device-class > summary svg { margin-left: auto; }
  .device-class[open] > summary svg { transform: rotate(90deg); }
  ul { list-style: none; padding: 0; margin: 0; display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 380px), 1fr)); column-gap: 28px; }
  li { display: grid; grid-template-columns: 32px minmax(0, 1fr) auto; column-gap: 12px; row-gap: 8px; padding: 20px 0; align-content: start; border-top: 1px solid var(--border); }
  .device-name, .device-version { display: flex; flex-direction: column; gap: 6px; }
  .device-name strong { font-size: 14px; font-weight: 600; overflow-wrap: anywhere; }
  .device-name span { font-size: 12px; color: var(--text-secondary); }
  .device-version strong { font-size: 12px; font-weight: 500; }
  .device-info { min-width: 0; font-size: 12px; }
  .device-info summary { cursor: pointer; }
  .device-info[open] { grid-column: 2 / -1; grid-row: 3; }
  dl { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 8px 16px; margin: 12px 0; }
  dt { color: var(--text-secondary); }
  dd { margin: 0; overflow-wrap: anywhere; }
  .device-problem { grid-column: 1 / -1; font-size: 12px; color: var(--warning); }
  .device-error { color: var(--danger); font-size: 13px; }
  .device-empty { color: var(--text-secondary); font-size: 13px; padding: 16px 0; }
  .device-logo { grid-column: 1; grid-row: 1 / span 2; color: var(--text-secondary); padding-top: 2px; }
  .device-name { grid-column: 2; grid-row: 1; }
  .device-version { grid-column: 2; grid-row: 2; color: var(--text-secondary); }
  .device-info { grid-column: 3; grid-row: 1 / span 2; }
  .device-info summary { display: flex; align-items: center; gap: 6px; list-style: none; min-height: 28px; }
  .device-info summary::-webkit-details-marker { display: none; }
  .device-info[open] summary :global(svg) { transform: rotate(180deg); }
  @container workspace (max-width: 520px) { .device-tools > input { flex-basis: 100%; } }

</style>
