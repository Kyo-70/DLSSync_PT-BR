<script lang="ts" module>
  /** Authoritative component readers for the list row.
   *
   *  Rust owns the update decision and the target version. Nothing here compares versions or
   *  resolves a target. `null` means Rust published no component state for that game, which the
   *  row presents as unknown rather than as "no updates". */
  import type { ComponentState, GameSnapshot } from "../generated/bindings";
  import type { DllRecord } from "../lib/api";
  import { recordFeature, type FeatureSlot } from "../lib/labels";

  /** Components Rust published for one game, or `null` while it published none. */
  export function publishedComponents(
    snapshots: GameSnapshot[],
    gameId: string,
  ): ComponentState[] | null {
    const snapshot = snapshots.find((candidate) => candidate.id === gameId);
    if (!snapshot) return null;
    return snapshot.components ?? null;
  }

  /** An update the surface may present: available, applicable, and with the candidate published. */
  export function isOfferedUpdate(component: ComponentState): boolean {
    return (
      component.status === "update_available" &&
      component.applicability !== "not_applicable" &&
      component.candidate !== null
    );
  }

  /** Feature slot of an authoritative component, resolved by family and filename. */
  export function componentFeature(component: ComponentState): FeatureSlot {
    return recordFeature({
      family: component.identity.family as DllRecord["family"],
      path: component.identity.filename,
      current_version: component.observed_version,
      file_description: null,
    });
  }
</script>

<script lang="ts">
  import type { DetectedGame } from "../lib/api";
  import {
    launcherLabel,
    featureShort,
    featureVendor,
    familyShort,
    type UpdateStatus,
  } from "../lib/labels";
  import { gameDllsLoading, gameStatuses, hardwarePreference } from "../lib/stores";
  import { preferredFamily } from "../lib/hardwarePreference";
  import { familyLabel } from "../lib/labels";
  import { authoritativeState } from "../lib/stateSync";
  import { launcherIcon } from "../lib/launcherIcons";
  import { bestArtSrc } from "../lib/gameArt";
  import { t } from "../lib/i18n/index";
  import { familyMeta } from "../lib/familyMeta";

  let { game, status: statusProp = null, hidden = false, favorite = false, onApply, onOpenFolder, onBlacklist, onClick, onContextMenu, onToggleFavorite }: {
    game: DetectedGame;
    /** Presentation status resolved by the owning view from the authoritative state. */
    status?: UpdateStatus | null;
    hidden?: boolean;
    favorite?: boolean;
    onApply: (g: DetectedGame) => void;
    onOpenFolder: (g: DetectedGame) => void;
    onBlacklist: (g: DetectedGame) => void;
    onClick: (g: DetectedGame) => void;
    onContextMenu?: (g: DetectedGame, e: MouseEvent) => void;
    onToggleFavorite?: (g: DetectedGame) => void;
  } = $props();

  // The row presents the status it is given; it does not classify the game itself.
  let status: UpdateStatus = $derived(statusProp ?? (($gameStatuses[game.id] ?? "unknown") as UpdateStatus));
  let loading = $derived($gameDllsLoading[game.id] ?? false);
  let imgErrored = $state(false);
  // Source of the drawn cover. Rust reports the verified asset and its dimensions; this only
  // picks the orientation and resolves a local cache file through the Tauri asset transport.
  const artHref = $derived(bestArtSrc(game));
  // Rust reports that this game has no cover, or that its source failed. The surface says so
  // instead of leaving a silent initial that looks like a loading state.
  const coverUnavailable = $derived(
    !artHref && (game.art?.state === "unavailable" || game.art?.state === "source_failed"),
  );
  let brandMark = $derived(launcherIcon(game.launcher));

  // Authoritative components for this game, or `null` while Rust published none for it.
  let components = $derived(publishedComponents($authoritativeState.games, game.id));
  let otherFamilies = $derived([...new Set((components ?? []).filter(component => !preferredFamily(component.identity.family, $hardwarePreference)).map(component => component.identity.family))]);
  let offeredUpdates = $derived(components ? components.filter(component => isOfferedUpdate(component) && preferredFamily(component.identity.family, $hardwarePreference)) : null);
  /** Pending updates Rust published, or `null` when it published no component state. */
  let outdatedCount = $derived<number | null>(offeredUpdates ? offeredUpdates.length : null);
  let primaryUpdate = $derived(offeredUpdates?.[0] ?? null);
  // Observed and target versions come from the same published component; the row resolves neither.
  let primaryObserved = $derived(primaryUpdate?.observed_version ?? null);
  let primaryTarget = $derived(primaryUpdate?.candidate?.package_version ?? null);

  let outdatedChips = $derived.by(() => {
    const seen = new Set<string>();
    const chips: { label: string; vendor: string }[] = [];
    for (const component of offeredUpdates ?? []) {
      const f = componentFeature(component);
      const key = f === "advanced" ? component.identity.family : f;
      if (seen.has(key)) continue;
      seen.add(key);
      chips.push({
        label: f === "advanced" ? familyShort(component.identity.family) : featureShort(f),
        vendor: f === "advanced" ? familyMeta(component.identity.family)?.vendor ?? "other" : featureVendor(f),
      });
    }
    return chips;
  });

  let visibleChips = $derived(outdatedChips.slice(0, 4));
  let overflowChips = $derived(Math.max(0, outdatedChips.length - visibleChips.length));


</script>

<div
  class="list-row"
  class:is-hidden={hidden}
  role="presentation"
  data-launcher={game.launcher}
  oncontextmenu={onContextMenu ? (e) => { e.preventDefault(); onContextMenu(game, e); } : undefined}
>
  <div class="cover">
    {#if artHref && !imgErrored}
      <img src={artHref} alt={game.name} loading="lazy" onerror={() => (imgErrored = true)} />
    {:else}
      <span class="cover-fallback" title={coverUnavailable ? $t("component.cover.unavailable") : undefined}>
        <span aria-hidden={coverUnavailable ? "true" : undefined}>{game.name.slice(0, 1).toUpperCase()}</span>
        {#if coverUnavailable}<span class="cover-unavailable-label">{$t("component.cover.unavailable")}</span>{/if}
      </span>
    {/if}
  </div>

  <div class="meta">
    <h3 class="row-name truncate" title={game.name}>
      <button
        class="row-name-btn truncate"
        aria-label={$t("component.card.rowAria", { name: game.name, status: $t("status." + status) })}
        onclick={() => onClick(game)}
      >{game.name}</button>
    </h3>
    <div class="row-context">
    <span class="launcher-chip chip">
      <svg viewBox={brandMark.viewBox} width="10" height="10" fill="currentColor" aria-hidden="true">
        <path d={brandMark.path} />
      </svg>
      {launcherLabel(game.launcher)}
    </span>
  <div class="status">
    {#if loading}
      <span class="chip chip-neutral">{$t("status.scanning")}</span>
    {:else if status === "outdated"}
      <!-- With no published component count the row says an update is available without inventing
           how many. -->
      <span class="chip chip-update is-strong"
        ><span class="state-dot" data-state="outdated" aria-hidden="true"></span
        >{outdatedCount === null
          ? $t("status.outdated")
          : outdatedCount === 0 ? $t("component.hardware.otherUpdates") : $t("component.card.updatesShort", { count: outdatedCount })}</span
      >
    {:else if status === "up_to_date"}
      <span class="chip chip-success"
        ><span class="state-dot" data-state="current" aria-hidden="true"></span
        >{$t("status.up_to_date")}</span
      >
    {:else if status === "scan_failed"}
      <span class="chip chip-danger">{$t("status.scan_failed")}</span>
    {:else if status === "no_dlls"}
      <span class="chip chip-neutral">{$t("status.no_dlls")}</span>
    {:else}
      <span class="chip chip-neutral">{$t("status.unknown")}</span>
    {/if}
  </div>
    </div>
  </div>

  <div class="chips">
    {#each visibleChips as c, i (i)}
      <span class="chip chip-vendor is-solid feature-chip" data-vendor={c.vendor}>{c.label}</span>
    {/each}
    {#if overflowChips > 0}
      <span class="feature-chip overflow">+{overflowChips}</span>
    {/if}
    {#if otherFamilies.length > 0}
      <details class="other-technologies"><summary>{$t("component.hardware.otherTechnologies")} ({otherFamilies.length})</summary><p>{otherFamilies.map(familyLabel).join(" · ")}</p></details>
    {/if}

  </div>

  <div class="version-diff mono" title={$t("component.card.versionDiffTitle")}>
    {#if primaryUpdate && primaryTarget}
      <span class="version-family">{familyShort(primaryUpdate.identity.family)}</span>
      <span class="version-values">
      <span class="ver-old">v{primaryObserved ?? "?"}</span>
      <span class="arrow" aria-hidden="true">→</span>
      <span class="ver-new">v{primaryTarget}</span>
      </span>
    {:else}
      <span class="ver-empty">—</span>
    {/if}
  </div>

  <div class="actions" onclick={(e) => e.stopPropagation()} role="presentation">
    {#if onToggleFavorite}
      <button
        class="btn btn-ghost btn-sm fav-row-btn"
        class:is-fav={favorite}
        aria-pressed={favorite}
        aria-label={favorite ? $t("component.card.unfavorite") : $t("component.card.favorite")}
        title={favorite ? $t("component.card.unfavorite") : $t("component.card.favorite")}
        onclick={() => onToggleFavorite?.(game)}
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill={favorite ? "currentColor" : "none"} stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>
      </button>
    {/if}
    {#if status === "outdated" && !hidden && outdatedCount !== 0}
      <button class="btn btn-primary btn-sm" onclick={() => onApply(game)} title={$t("component.card.applyLatest")}>
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
        {$t($hardwarePreference.known ? "common.apply" : "component.hardware.reviewUpdates")}
      </button>
    {/if}
    <button class="btn btn-ghost btn-sm" onclick={() => onOpenFolder(game)} title={$t("component.card.openInstallFolder")} aria-label={$t("component.card.openInstallFolder")}>
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
    </button>
    <button class="btn btn-ghost btn-sm" onclick={() => onBlacklist(game)} title={hidden ? $t("component.card.restoreToLibrary") : $t("component.card.hideFromList")} aria-label={hidden ? $t("common.restore") : $t("component.card.hide")}>
      {#if hidden}
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.85.93 6.63 2.46"/><polyline points="21 4 21 9 16 9"/></svg>
      {:else}
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="4.93" y1="4.93" x2="19.07" y2="19.07"/></svg>
      {/if}
    </button>
  </div>
</div>

<style>
  .list-row {
    position: relative;
    display: grid;
    grid-template-columns: 112px minmax(180px, 1.3fr) minmax(100px, 1fr) minmax(130px, 1fr) auto;
    grid-template-areas: "cover meta chips version actions";
    align-items: center;
    gap: 20px;
    padding: 22px 20px;
    background: var(--bg-card);
    border: 0;
    border-bottom: 1px solid var(--border);
    border-radius: 0;
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease);
    color: var(--text-primary);
  }
  .list-row:hover { background: var(--bg-card-hover); border-color: var(--border-hover); }
  .list-row.is-hidden { opacity: 1; }
  .row-name-btn {
    display: block;
    max-width: 100%;
    padding: 0;
    background: transparent;
    border: none;
    font: inherit;
    color: inherit;
    letter-spacing: inherit;
    text-align: left;
    cursor: pointer;
  }
  .row-name-btn::after {
    content: "";
    position: absolute;
    inset: 0;
    z-index: 1;
  }
  .row-name-btn:focus-visible { outline: 2px solid var(--accent); outline-offset: 3px; border-radius: 3px; }

  .cover {
    grid-area: cover;
    width: 112px;
    height: 76px;
    overflow: hidden;
    border-radius: var(--radius-sm);
    background: var(--bg-art-fallback);
    position: relative;
    flex-shrink: 0;
  }
  .cover img { width: 100%; height: 100%; object-fit: cover; }
  .cover-fallback {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 16px;
    font-weight: 700;
    color: var(--launcher-accent, var(--accent));
    opacity: 0.7;
  }
  .cover-unavailable-label {
    position: absolute;
    bottom: 3px;
    left: 50%;
    transform: translateX(-50%);
    max-width: calc(100% - 6px);
    padding: 1px 4px;
    border-radius: var(--radius-full);
    background: color-mix(in oklab, var(--bg-elevated) 88%, transparent);
    color: var(--text-secondary);
    font-size: 8px;
    font-weight: 600;
    line-height: 1.1;
    text-align: center;
    white-space: nowrap;
  }

  .meta { grid-area: meta; display: flex; flex-direction: column; gap: 10px; min-width: 0; }
  .row-context { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 12px; }
  .row-name {
    font-size: 15px;
    font-weight: 600;
    letter-spacing: var(--letter-tight);
  }
  .launcher-chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--text-secondary);
    text-transform: none;
    letter-spacing: 0;
    width: fit-content;
    border: 0;
    background: none;
    padding: 0;
    font-size: 12px;
    font-weight: 400;
  }

  .status { min-width: 0; }
  .status :global(.chip) { text-transform: none; font-size: 12px; letter-spacing: 0; padding: 0; background: none; border: 0; color: var(--text-secondary); font-weight: 500; }

  .chips { grid-area: chips; display: flex; flex-wrap: wrap; align-items: center; gap: 6px; min-width: 0; }
  .feature-chip.chip { white-space: normal; padding: 4px 7px; border: 1px solid var(--border); background: var(--bg-elevated); color: var(--text-secondary); text-transform: none; letter-spacing: 0; font-size: 12px; font-weight: 500; }
  .feature-chip.overflow {
    display: inline-flex;
    align-items: center;
    padding: 3px 8px;
    font-size: 10.5px;
    font-weight: 600;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-muted);
    border: 1px solid var(--border);
  }

  .version-diff {
    grid-area: version;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 7px;
    font-size: var(--fs-sm);
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
    white-space: normal;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .version-family { font-family: var(--font-sans); font-size: 12px; color: var(--text-muted); }
  .version-values { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; font-size: 12px; }
  .ver-old { color: var(--text-muted); }
  .ver-new { color: var(--update); font-weight: 600; }
  .arrow { color: var(--text-muted); }
  .ver-empty { color: var(--text-placeholder); }

  .actions {
    grid-area: actions;
    position: relative;
    z-index: 2;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    justify-content: flex-end;
    flex-shrink: 0;
  }
  .fav-row-btn.is-fav { color: var(--gh-star); }
  .fav-row-btn.is-fav:hover { color: var(--gh-star); }

  @container workspace (max-width: 1000px) {
    .list-row { grid-template-columns: 96px minmax(0, 1fr) auto; grid-template-areas: "cover meta actions" "cover chips version"; gap: 14px 18px; }
    .cover { width: 96px; height: 72px; }
  }
  @container workspace (max-width: 560px) {
    .list-row { grid-template-columns: 64px minmax(0, 1fr); grid-template-areas: "cover meta" "chips chips" "version actions"; gap: 16px 12px; padding: 18px 14px; }
    .cover { width: 64px; height: 52px; }
    .actions { flex-wrap: wrap; gap: 4px; }
    .row-name { white-space: normal; }
    .row-name-btn { white-space: normal; }
  }

  .other-technologies { position: relative; z-index: 2; font-size: 12px; color: var(--text-muted); line-height: 1.5; }
  .other-technologies summary { cursor: pointer; }
  .other-technologies p { margin: 6px 0 0; overflow-wrap: anywhere; }
</style>
