<script module lang="ts">
  /** Authoritative readers for the Library surface.
   *
   *  Every function here READS a value Rust published. None of them compares versions, resolves a
   *  target version, or decides compatibility, trust or install success. `unchecked`, `unknown`,
   *  `no_components` and `non_actionable` stay distinct from `update_available`: none of them is
   *  presented as an available update, and none of them joins the apply set. */
  import type { DllRecord, GameSnapshot, LauncherKind, OperationSnapshot, StateCounts } from "../lib/api";
  import type { ComponentState, GameStateStatus } from "../generated/bindings";
  import type { UpdateStatus } from "../lib/labels";
  import { gameOperationLabel as operationLabel } from "../lib/labels";
  import type { ApplyTarget } from "../lib/applyController";
  import type { OutdatedDllItem } from "../lib/stores";

  /** Rust publishes the component path relative to the game install root, with `/` separators. */
  function componentPath(installDir: string, relativePath: string): string {
    const root = installDir.replace(/[\\/]+$/, "");
    const relative = relativePath.replace(/^[\\/]+/, "").replaceAll("/", "\\");
    return `${root}\\${relative}`;
  }

  /** One apply target built from an authoritative component, or `null` when Rust published no
   *  candidate for it. The target version is the candidate's package version as published; the
   *  view never derives it. */
  function targetFromComponent(game: GameSnapshot, component: ComponentState): ApplyTarget | null {
    if (component.status !== "update_available" || component.applicability === "not_applicable") return null;
    const candidate = component.candidate;
    if (candidate === null) return null;
    const observedHash = component.observed_hash;
    return {
      game_id: game.id,
      game_label: operationLabel((game.launcher ?? "manual") as LauncherKind, game.name),
      target_version: candidate.package_version,
      catalog_family: candidate.family,
      record: {
        family: component.identity.family as DllRecord["family"],
        path: componentPath(game.install_dir, component.identity.relative_path),
        current_version: component.observed_version,
        sha256: observedHash !== null && observedHash.algorithm === "sha256" ? observedHash.digest : null,
        file_description: null,
      },
    };
  }

  /** Membership of the "update all" set, read from the authoritative snapshot. Hiding a game stays
   *  a local presentation preference, so it is the only frontend filter applied here. */
  export function authoritativeApplyTargets(
    snapshotGames: GameSnapshot[],
    hidden: ReadonlySet<string>,
  ): ApplyTarget[] {
    const targets: ApplyTarget[] = [];
    for (const game of snapshotGames) {
      if (hidden.has(game.id)) continue;
      if (game.status !== "update_available") continue;
      for (const component of game.components ?? []) {
        const target = targetFromComponent(game, component);
        if (target !== null) targets.push(target);
      }
    }
    return targets;
  }

  /** Shape adapter for the shared store helper used while Rust publishes no game observation.
   *  The comparison and the target version are owned by that helper, not by this view. */
  export function targetsFromStoreItems(items: OutdatedDllItem[]): ApplyTarget[] {
    return items.map((item) => ({
      game_id: item.game.id,
      game_label: operationLabel(item.game.launcher, item.game.name),
      record: item.record,
      target_version: item.target,
      catalog_family: item.catalogFamily,
    }));
  }

  /** Presentation status for one authoritative game state. The four non-actionable states keep
   *  their own presentation and never read as "update available". */
  export function presentedGameStatus(status: GameStateStatus | undefined): UpdateStatus {
    switch (status) {
      case "update_available":
        return "outdated";
      case "current":
        return "up_to_date";
      case "no_components":
        return "no_dlls";
      default:
        return "unknown";
    }
  }

  /** Games Rust reports as current, minus the ones the user hides locally. */
  export function upToDateGameCount(
    snapshotGames: GameSnapshot[],
    hidden: ReadonlySet<string>,
  ): number {
    return snapshotGames.reduce(
      (total, game) => (!hidden.has(game.id) && game.status === "current" ? total + 1 : total),
      0,
    );
  }

  /** Highest observation revision Rust published for the games an apply touched, taken from the
   *  operation that carries them and from the game snapshots themselves. `null` while no such
   *  revision is installed. The view waits for this instead of rescanning each game. */
  export function applyObservationRevision(
    snapshotGames: GameSnapshot[],
    operations: OperationSnapshot[],
    gameIds: ReadonlySet<string>,
  ): string | null {
    if (gameIds.size === 0) return null;
    let highest: bigint | null = null;
    let raw: string | null = null;
    const consider = (value: string | undefined | null): void => {
      if (value === undefined || value === null || value === "") return;
      let parsed: bigint;
      try {
        parsed = BigInt(value);
      } catch {
        return;
      }
      if (highest === null || parsed > highest) {
        highest = parsed;
        raw = value;
      }
    };
    for (const operation of operations) {
      if (!(operation.game_ids ?? []).some((id) => gameIds.has(id))) continue;
      consider(operation.state_revision);
    }
    for (const game of snapshotGames) {
      if (gameIds.has(game.id)) consider(game.revision);
    }
    return raw;
  }

  /** Games carrying a restore point, as projected by Rust. `null` while the projection is absent:
   *  the view does not infer protection from backup rows. */
  export function projectedProtectedGames(counts: StateCounts | null): number | null {
    const projected = (counts as (StateCounts & { protected_games?: number }) | null)?.protected_games;
    return typeof projected === "number" ? projected : null;
  }
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import {
    games,
    filteredGames,
    scanInProgress,
    scanGames,
    searchQuery,
    launcherFilter,
    statusFilter,
    showToast,
    drawerGameId,
    settings,
    persistSettings,
    persistUiPreferences,
    hardwarePreference,
    gameDlls,
    gameStatuses,
    relationContext,
    activeApplies,
    applyModalOpen,
    hiddenIds,
    favoriteIds,
    toggleFavorite,
    technologyFilter,
    antiCheatFilter,
    rescanGame,
    requestApplyAllOutdated,
    outdatedDllItems,
    loadBackups,
    type StatusFilter,
  } from "../lib/stores";
  import { authoritativeState } from "../lib/stateSync";
  import { addBlacklistEntry, removeBlacklistEntry, openPath } from "../lib/api";
  import type { DetectedGame, LibraryViewMode, LibraryDensity, LibrarySort } from "../lib/api";
  import {
    LIBRARY_SORT_LABELS,
    LIBRARY_VIEW_MODE_DEFAULT,
    LIBRARY_DENSITY_DEFAULT,
    LIBRARY_SORT_DEFAULT,
  } from "../lib/ux";
  import { GROUP_LABELS, GROUP_ORDER } from "../lib/labels";
  import { defaultUpdateFamily } from "../lib/hardwarePreference";
  import type { TechnologyFilter, AntiCheatFilter } from "../lib/libraryFilters";
  import GameCard from "../components/GameCard.svelte";
  import GameListRow from "../components/GameListRow.svelte";
  import FilterMenu from "../components/FilterMenu.svelte";
  import ContextMenu, { type ContextMenuAction, type ContextMenuItem } from "../components/ContextMenu.svelte";
  import { dispatchApply } from "../lib/applyController";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { TRAY_SHOW_PROGRESS_EVENT } from "../lib/api";
  import { t, locale, translate } from "../lib/i18n/index";
  import { get } from "svelte/store";

  // Rust owns which components have an applicable update and which version they target. This view
  // reads that set; when the backend has published no game observation yet it delegates to the
  // shared store helper, which is also what the digest and the background daemon consume.
  let authoritativeGames = $derived($authoritativeState.games);
  let hasAuthoritativeGames = $derived($authoritativeState.emitterId !== null);
  let backendApplyTargets = $derived.by(() => {
    if (hasAuthoritativeGames) return authoritativeApplyTargets(authoritativeGames, $hiddenIds);
    void $games;
    void $gameDlls;
    void $gameStatuses;
    void $relationContext;
    void $settings;
    void $hiddenIds;
    return targetsFromStoreItems(outdatedDllItems("all"));
  });
  let applyAllTargets = $derived(backendApplyTargets.filter(target => defaultUpdateFamily(target.record.family, $hardwarePreference)));
  let recommendedGameIds = $derived(new Set(applyAllTargets.map(target => target.game_id)));
  let outdatedTotal = $derived(applyAllTargets.length);



  // Games touched by the last dispatch, and the observation revision Rust published for them. The
  // view re-renders from that revision; it never rescans a game speculatively after an apply.
  let lastApplyGameIds = $state<ReadonlySet<string>>(new Set<string>());
  let lastApplyObservationRevision = $derived(
    applyObservationRevision($authoritativeState.games, $authoritativeState.operations, lastApplyGameIds),
  );

  async function updateAllOutdated(): Promise<void> {
    const targets = applyAllTargets;
    if (targets.length === 0) {
      if (!$hardwarePreference.known || backendApplyTargets.length > 0) {
        showToast("info", $t(!$hardwarePreference.known ? "component.hardware.unknown" : "component.hardware.noRecommended"));
        return;
      }
      showToast(
        "info",
        translate(
          get(locale),
          hasAuthoritativeGames || $games.length > 0
            ? "view.library.toast.allUpToDate"
            : "status.unknown",
        ),
      );
      return;
    }
    lastApplyGameIds = new Set(targets.map((target) => target.game_id));
    await dispatchApply(targets, { showModal: () => applyModalOpen.set(true) });
    // No per-game rescan here: Rust publishes the post-apply observation for the affected games and
    // this view updates when that revision arrives.
  }

  let unlistenTrayProgress: UnlistenFn | undefined;
  onMount(() => {
    if ($games.length === 0) {
      void scanGames();
    }
    void loadBackups();
    void listen<void>(TRAY_SHOW_PROGRESS_EVENT, () => {
      if (Object.keys($activeApplies).length > 0) {
        applyModalOpen.set(true);
      }
    }).then((un) => {
      unlistenTrayProgress = un;
    });
    return () => unlistenTrayProgress?.();
  });

  const launcherFilters = [
    { id: "all", labelKey: "view.library.launcherFilter.all", brand: null },
    { id: "steam", labelKey: null, brand: "Steam" },
    { id: "epic", labelKey: null, brand: "Epic" },
    { id: "gog", labelKey: null, brand: "GOG" },
    { id: "ubisoft", labelKey: null, brand: "Ubisoft" },
    { id: "ea_desktop", labelKey: null, brand: "EA" },
    { id: "xbox", labelKey: null, brand: "Xbox" },
    { id: "battlenet", labelKey: null, brand: "Battle.net" },
    { id: "manual", labelKey: "view.library.launcherFilter.custom", brand: null },
  ] as const;

  const statusFilters: { id: StatusFilter; labelKey: string }[] = [
    { id: "all", labelKey: "view.library.statusFilter.all" },
    { id: "favorite", labelKey: "view.library.statusFilter.favorite" },
    { id: "outdated", labelKey: "status.outdated" },
    { id: "up_to_date", labelKey: "status.up_to_date" },
    { id: "no_dlls", labelKey: "status.no_dlls" },
    { id: "scan_failed", labelKey: "status.scan_failed" },
    { id: "hidden", labelKey: "view.library.statusFilter.hidden" },
  ];

  let hiddenCount = $derived($hiddenIds.size);
  let favoriteCount = $derived($favoriteIds.size);

  function onToggleFav(game: DetectedGame): void {
    void toggleFavorite(game.id);
  }

  let technologyOptions = $derived([
    { id: "all", label: translate($locale, "view.library.launcherFilter.all") },
    ...GROUP_ORDER.map((g) => ({ id: g as string, label: GROUP_LABELS[g] })),
  ]);
  let antiCheatOptions = $derived([
    { id: "all", label: translate($locale, "view.library.launcherFilter.all") },
    { id: "flagged", label: translate($locale, "view.library.filter.antiCheatFlagged"), tone: "danger" as const },
    { id: "clear", label: translate($locale, "view.library.filter.antiCheatClear") },
  ]);

  let launcherOptions = $derived.by(() =>
    launcherFilters
      .map((f) => ({
        id: f.id,
        label: f.brand ?? translate($locale, f.labelKey),
        count: f.id === "all" ? $games.length : $games.filter((g) => g.launcher === f.id).length,
      }))
      .filter((o) => o.id === "all" || availableLaunchers.has(o.id) || o.count > 0),
  );

  let statusOptions = $derived.by(() =>
    statusFilters
      .filter((f) => (f.id !== "hidden" || hiddenCount > 0) && (f.id !== "favorite" || favoriteCount > 0))
      .map((f) => ({
        id: f.id,
        label: translate($locale, f.labelKey),
        count: f.id === "hidden" ? hiddenCount : f.id === "favorite" ? favoriteCount : undefined,
        tone: f.id === "hidden" ? ("danger" as const) : null,
      })),
  );

  function onCardClick(game: DetectedGame): void {
    drawerGameId.set(game.id);
  }
  async function onApply(game: DetectedGame): Promise<void> {
    const targets = applyAllTargets.filter(target => target.game_id === game.id);
    if (!targets.length) { drawerGameId.set(game.id); return; }
    lastApplyGameIds = new Set([game.id]);
    await dispatchApply(targets, { showModal: () => applyModalOpen.set(true) });
  }
  async function onOpenFolder(game: DetectedGame): Promise<void> {
    try {
      await openPath(game.install_dir);
    } catch (err: unknown) {
      showToast(
        "danger",
        translate(get(locale), "view.library.toast.openFolderFailed", { error: String(err) }),
      );
    }
  }
  async function onHideToggle(game: DetectedGame): Promise<void> {
    const wasHidden = $hiddenIds.has(game.id);
    try {
      const next = wasHidden
        ? await removeBlacklistEntry(game.id)
        : await addBlacklistEntry(game.id);
      if ($settings) settings.set({ ...$settings, blacklist: next });
      showToast(
        wasHidden ? "success" : "info",
        translate(
          get(locale),
          wasHidden ? "view.library.toast.gameRestored" : "view.library.toast.gameHidden",
          { name: game.name },
        ),
      );
    } catch (err: unknown) {
      showToast(
        "danger",
        translate(
          get(locale),
          wasHidden ? "view.library.toast.restoreFailed" : "view.library.toast.hideFailed",
          { error: String(err) },
        ),
      );
    }
  }

  let contextMenu = $state<{ game: DetectedGame; x: number; y: number } | null>(null);
  let contextMenuItems = $derived.by<ContextMenuItem[]>(() => {
    if (!contextMenu) return [];
    const isHidden = $hiddenIds.has(contextMenu.game.id);
    const isFav = $favoriteIds.has(contextMenu.game.id);
    const tr = (key: string): string => translate(get(locale), key);
    return [
      { action: "open_folder", label: tr("view.library.menu.openFolder") },
      { action: "scan", label: tr("view.library.menu.scan") },
      { action: "favorite", label: tr(isFav ? "component.card.unfavorite" : "component.card.favorite") },
      { action: "hide", label: tr(isHidden ? "view.library.menu.unhide" : "view.library.menu.hide") },
    ];
  });

  function openContextMenu(game: DetectedGame, e: MouseEvent): void {
    contextMenu = { game, x: e.clientX, y: e.clientY };
  }

  async function onContextSelect(action: ContextMenuAction): Promise<void> {
    const game = contextMenu?.game;
    if (!game) return;
    switch (action) {
      case "open_folder":
        await onOpenFolder(game);
        break;
      case "scan":
        await rescanGame(game.id);
        showToast(
          "info",
          translate(get(locale), "view.library.toast.rescanning", { name: game.name }),
        );
        break;
      case "favorite":
        await toggleFavorite(game.id);
        break;
      case "hide":
        await onHideToggle(game);
        break;
    }
  }

  async function addCustomFolder(): Promise<void> {
    if (!$settings) return;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const result = await open({ directory: true, multiple: false });
      if (typeof result !== "string" || !result) return;
      if ($settings.launcher_overrides.custom.includes(result)) {
        showToast("warning", translate(get(locale), "view.library.toast.folderAlreadyAdded"));
        return;
      }
      await persistSettings({
        ...$settings,
        launcher_overrides: {
          ...$settings.launcher_overrides,
          custom: [...$settings.launcher_overrides.custom, result],
        },
      });
      showToast("success", translate(get(locale), "view.library.toast.folderAdded", { path: result }));
      await scanGames({ trigger: "user_scan" });
    } catch (err: unknown) {
      showToast(
        "danger",
        translate(get(locale), "view.library.toast.folderPickerFailed", { error: String(err) }),
      );
    }
  }

  let filtersOpen = $state(false);
  let activeFilterCount = $derived(Number($launcherFilter !== "all") + Number($technologyFilter !== "all") + Number($antiCheatFilter !== "all") + Number(!["all", "outdated", "hidden"].includes($statusFilter)));
  let availableLaunchers = $derived(new Set($games.map((g) => g.launcher)));

  let viewMode: LibraryViewMode = $derived(($settings?.ui_prefs.library_view_mode ?? LIBRARY_VIEW_MODE_DEFAULT) as LibraryViewMode);
  let density: LibraryDensity = $derived(($settings?.ui_prefs.library_density ?? LIBRARY_DENSITY_DEFAULT) as LibraryDensity);
  let sortKey: LibrarySort = $derived(($settings?.ui_prefs.library_sort ?? LIBRARY_SORT_DEFAULT) as LibrarySort);

  async function setPresentation(mode: "gallery" | "compact" | "table"): Promise<void> {
    if (!$settings) return;
    await persistUiPreferences({
      library_view_mode: mode === "table" ? "list" : "grid",
      library_density: mode === "compact" ? "compact" : "comfy",
    });
  }

  async function setSort(s: LibrarySort): Promise<void> {
    if (!$settings) return;
    await persistUiPreferences({ library_sort: s });
  }

  const STATUS_SORT_RANK: Record<string, number> = {
    outdated: 0,
    scan_failed: 1,
    up_to_date: 2,
    no_dlls: 3,
    unknown: 4,
    scanning: 5,
  };

  // One presentation status per game. Rust's published state wins; the legacy store map is the
  // fallback while the backend publishes no game observation.
  let presentedStatusById = $derived.by<Record<string, UpdateStatus>>(() => {
    const byId: Record<string, UpdateStatus> = {};
    for (const snapshot of authoritativeGames) byId[snapshot.id] = presentedGameStatus(snapshot.status);
    return byId;
  });
  function statusOf(game: DetectedGame): UpdateStatus {
    return presentedStatusById[game.id] ?? (hasAuthoritativeGames ? "unknown" : (($gameStatuses[game.id] ?? "unknown") as UpdateStatus));
  }

  function byOutdatedThenName(a: DetectedGame, b: DetectedGame): number {
    const ra = STATUS_SORT_RANK[statusOf(a)] ?? 9;
    const rb = STATUS_SORT_RANK[statusOf(b)] ?? 9;
    return ra - rb || a.name.localeCompare(b.name);
  }

  let sortedGames = $derived.by(() => {
    const list = [...$filteredGames];
    switch (sortKey) {
      case "a_z":
        return list.sort((a, b) => a.name.localeCompare(b.name));
      case "z_a":
        return list.sort((a, b) => b.name.localeCompare(a.name));
      case "launcher":
        return list.sort((a, b) => a.launcher.localeCompare(b.launcher) || a.name.localeCompare(b.name));
      case "outdated_first":
      case "default":
      default:
        return list.sort(byOutdatedThenName);
    }
  });

  let visibleGames = $derived($statusFilter === "outdated" && $hardwarePreference.known ? sortedGames.filter(game => recommendedGameIds.has(game.id)) : sortedGames);
  let otherUpdateGames = $derived($statusFilter === "outdated" && $hardwarePreference.known ? sortedGames.filter(game => !recommendedGameIds.has(game.id)) : []);

  function reviewChanges(): void {
    launcherFilter.set("all");
    statusFilter.set("outdated");
    searchQuery.set("");
  }

  function revealHidden(): void {
    launcherFilter.set("all");
    statusFilter.set("hidden");
    searchQuery.set("");
  }

  let lastApplyAllSignal = $state(0);
  $effect(() => {
    const n = $requestApplyAllOutdated;
    if (n !== lastApplyAllSignal) {
      lastApplyAllSignal = n;
      if (n > 0) void updateAllOutdated();
    }
  });
</script>

<header class="library-masthead">
  <div class="library-heading">
    <h1 class="view-title">{$t("view.library.title")}</h1>
    <div class="library-summary" role="status" aria-label={$t("view.library.hero.aria")} data-update-count={outdatedTotal} data-observation-revision={lastApplyObservationRevision ?? undefined}>
      {#if $scanInProgress && $games.length === 0}
        <span class="spin"></span><span>{$t("status.scanning")}</span>
      {:else}
        <span>{$t("view.library.gameCount", { count: $filteredGames.length })}</span>
        {#if outdatedTotal > 0}<span class="summary-separator" aria-hidden="true">·</span><span>{$t("view.library.summaryUpdates", { count: outdatedTotal, games: recommendedGameIds.size })}</span>{/if}
      {/if}
    </div>
  </div>
  <div class="header-actions">
    <button class="btn btn-ghost" onclick={addCustomFolder}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/><line x1="12" y1="11" x2="12" y2="17"/><line x1="9" y1="14" x2="15" y2="14"/></svg>
      {$t("view.library.addFolder")}
    </button>
    <button class="btn" disabled={$scanInProgress} onclick={() => scanGames({ trigger: "user_scan" })}>
      {#if $scanInProgress}
        <span class="spin"></span>
        {$t("view.library.scanning")}
      {:else}
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/></svg>
        {$t("view.library.rescan")}
      {/if}
    </button>
    {#if outdatedTotal > 0}
      <button class="btn btn-primary" data-testid="library-update-all" onclick={updateAllOutdated} title={$t("view.library.hero.applyAllTitle")}>{$t("view.library.hero.applyAll")}</button>
    {/if}
  </div>
</header>

<div class="library-workbar">
  <nav class="library-quick-filters" aria-label={$t("view.library.filter.status")}>
    <button class:active={$statusFilter === "all"} aria-pressed={$statusFilter === "all"} onclick={() => statusFilter.set("all")}>{$t("view.library.statusFilter.all")}</button>
    <button class:active={$statusFilter === "outdated"} aria-pressed={$statusFilter === "outdated"} onclick={reviewChanges}>{$t("view.library.updatesTab")}<span>{recommendedGameIds.size}</span></button>
    {#if hiddenCount > 0}<button class:active={$statusFilter === "hidden"} aria-pressed={$statusFilter === "hidden"} onclick={revealHidden}>{$t("view.library.statusFilter.hidden")}<span>{hiddenCount}</span></button>{/if}
  </nav>
  <div class="library-view-controls">
    <button class="library-filter-toggle" aria-expanded={filtersOpen} onclick={() => (filtersOpen = !filtersOpen)}>
      <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M4 7h16M4 17h16M8 4v6M16 14v6"/></svg>
      {$t("view.library.filtersTitle")}{#if activeFilterCount > 0}<span>{activeFilterCount}</span>{/if}
    </button>
    <FilterMenu iconOnly label={$t("view.library.filter.sort")} selectedId={sortKey === "default" ? "outdated_first" : sortKey}
      options={LIBRARY_SORT_LABELS.filter((option) => option.id !== "default").map((option) => ({ id: option.id, label: $t("librarySort." + option.id + ".label") }))}
      onSelect={(value) => void setSort(value as LibrarySort)} />
    <div class="presentation-picker" role="group" aria-label={$t("view.library.filter.view")}>
      <button data-testid="view-gallery" class:active={viewMode === "grid" && density === "comfy"} aria-pressed={viewMode === "grid" && density === "comfy"} onclick={() => void setPresentation("gallery")} title={$t("view.library.view.gridTitle")} aria-label={$t("view.library.view.grid")}>
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><rect x="3" y="3" width="7" height="18" rx="1.5"/><rect x="14" y="3" width="7" height="18" rx="1.5"/></svg>
      </button>
      <button data-testid="view-compact" class:active={viewMode === "grid" && density === "compact"} aria-pressed={viewMode === "grid" && density === "compact"} onclick={() => void setPresentation("compact")} title={$t("view.library.density.compactTitle")} aria-label={$t("view.library.density.compact")}>
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/></svg>
      </button>
      <button data-testid="view-table" class:active={viewMode === "list"} aria-pressed={viewMode === "list"} onclick={() => void setPresentation("table")} title={$t("view.library.view.listTitle")} aria-label={$t("view.library.view.list")}>
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="1.5"/><path d="M3 9h18M3 14h18M9 4v16"/></svg>
      </button>
    </div>
  </div>
</div>
{#if filtersOpen}
  <div class="library-filter-options" role="group" aria-label={$t("view.library.filtersTitle")}>
  <FilterMenu
    label={$t("view.library.filter.launcher")}
    options={launcherOptions}
    selectedId={$launcherFilter}
    onSelect={(id) => launcherFilter.set(id)}
  />

  <FilterMenu
    label={$t("view.library.filter.status")}
    options={statusOptions}
    selectedId={$statusFilter}
    onSelect={(id) => statusFilter.set(id as StatusFilter)}
  />

  <FilterMenu
    label={$t("view.library.filter.technology")}
    options={technologyOptions}
    selectedId={$technologyFilter}
    onSelect={(id) => technologyFilter.set(id as TechnologyFilter)}
  />

  <FilterMenu
    label={$t("view.library.filter.antiCheat")}
    options={antiCheatOptions}
    selectedId={$antiCheatFilter}
    onSelect={(id) => antiCheatFilter.set(id as AntiCheatFilter)}
  />

  </div>
{/if}

{#if $scanInProgress && $games.length === 0}
  <div class="grid">
    {#each Array(8) as _, i (i)}
      <div class="card-skel">
        <div class="skel-art skeleton"></div>
        <div class="skel-body">
          <div class="skel-line skel-line-lg skeleton"></div>
          <div class="skel-line skel-line-sm skeleton"></div>
        </div>
      </div>
    {/each}
  </div>
{:else if $games.length === 0}
  <div class="empty">
    <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
    <h3 class="empty-title">{$t("view.library.empty.noGames.title")}</h3>
    <p class="section-sub">{$t("view.library.empty.noGames.detail")}</p>
    <p class="section-sub">{$t("view.library.empty.noGames.customPrefix")} <span class="mono">C:\Games</span>.</p>
    <div class="empty-actions">
      <button class="btn btn-primary" disabled={$scanInProgress} onclick={() => scanGames({ trigger: "user_scan" })}>
        {#if $scanInProgress}<span class="spin"></span>{$t("view.library.scanning")}{:else}{$t("view.library.empty.noGames.rescanNow")}{/if}
      </button>
      <button class="btn btn-ghost" onclick={addCustomFolder}>{$t("view.library.empty.noGames.addCustomFolder")}</button>
    </div>
  </div>
{:else if $filteredGames.length === 0}
  <div class="empty">
    <h3 class="empty-title">{$t("view.library.empty.noMatch.title")}</h3>
    <p class="section-sub">{$t("view.library.empty.noMatch.detail")}</p>
    <button class="btn btn-accent" onclick={() => { searchQuery.set(""); launcherFilter.set("all"); statusFilter.set("all"); technologyFilter.set("all"); antiCheatFilter.set("all"); }}>{$t("view.library.empty.noMatch.reset")}</button>
  </div>
{:else}
  {#snippet gameSection(title: string, list: DetectedGame[], viewAll: StatusFilter)}
    {#if list.length > 0}
      <section class="lib-section" data-section={viewAll}>
        {#if title}<div class="section-head">
          <span class="section-title">{title}</span>
          <span class="section-count">{list.length}</span>
          {#if $statusFilter === "all"}
            <button class="section-viewall" onclick={() => statusFilter.set(viewAll)}>{$t("view.library.section.viewAll")}</button>
          {/if}
        </div>{/if}
        {#if viewMode === "grid"}
          <div class="grid media-deck" data-density={density}>
            {#each list as g, i (g.install_dir)}
              <div class="grid-cell media-card" style:--stagger="{Math.min(i, 20) * 24}ms">
                <GameCard
                  coverMode={density === "comfy" ? "portrait" : "landscape"}
                  game={g}
                  status={statusOf(g)}
                  hidden={$hiddenIds.has(g.id)}
                  favorite={$favoriteIds.has(g.id)}
                  {onApply}
                  {onOpenFolder}
                  onBlacklist={onHideToggle}
                  onClick={onCardClick}
                  onContextMenu={openContextMenu}
                  onToggleFavorite={onToggleFav}
                />
              </div>
            {/each}
          </div>
        {:else}
          <div class="list">
            {#each list as g, i (g.install_dir)}
              <div class="list-cell" style:--stagger="{Math.min(i, 20) * 12}ms">
                <GameListRow
                  game={g}
                  status={statusOf(g)}
                  hidden={$hiddenIds.has(g.id)}
                  favorite={$favoriteIds.has(g.id)}
                  {onApply}
                  {onOpenFolder}
                  onBlacklist={onHideToggle}
                  onClick={onCardClick}
                  onContextMenu={openContextMenu}
                  onToggleFavorite={onToggleFav}
                />
              </div>
            {/each}
          </div>
        {/if}
      </section>
    {/if}
  {/snippet}

  {@render gameSection("", visibleGames, $statusFilter)}
  {#if otherUpdateGames.length > 0}
    <details class="other-update-games">
      <summary>{$t("component.hardware.otherTechnologies")} · {otherUpdateGames.length}</summary>
      <p>{$t("component.hardware.otherHelp")}</p>
      {@render gameSection("", otherUpdateGames, "outdated")}
    </details>
  {/if}

{/if}

{#if contextMenu}
  <ContextMenu
    x={contextMenu.x}
    y={contextMenu.y}
    items={contextMenuItems}
    onSelect={(a) => void onContextSelect(a)}
    onClose={() => (contextMenu = null)}
  />
{/if}

<style>


  .header-actions { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; flex-shrink: 0; }










  /* Once the four filters + the controls can no longer share one row, drop the
     controls to their own full-width row (left-aligned) instead of overflowing. */
  @container (max-width: 900px) {

  }
  @container (max-width: 460px) {

  }






























  @container (max-width: 760px) {



  }
  @container (max-width: 620px) {


  }




  .lib-section { margin-bottom: 30px; }
  .lib-section:last-of-type { margin-bottom: 8px; }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(248px, 1fr));
    gap: 16px;
    padding-bottom: 32px;
  }
  .lib-section .grid { padding-bottom: 4px; }
  .grid[data-density="compact"] {
    grid-template-columns: repeat(auto-fill, minmax(184px, 1fr));
    gap: 10px;
  }
  .grid[data-density="compact"] :global(.body) { padding: 9px 11px 11px; gap: 6px; }
  .grid[data-density="compact"] :global(.game-name) { font-size: 12.5px; }
  .grid[data-density="compact"] :global(.feature-chip) { padding: 2px 6px 2px 5px; font-size: 10px; }
  .grid[data-density="compact"] :global(.launcher-text) { display: none; }
  .grid[data-density="compact"] :global(.launcher-chip) { padding: 4px; }
  @media (max-width: 720px) {
    .grid { grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 12px; }
  }
  .grid-cell {
    opacity: 0;
    transform: translateY(8px);
    animation: cellIn var(--dur-slow) var(--ease-out) forwards;
    animation-delay: var(--stagger, 0ms);
  }
  @keyframes cellIn {
    to { opacity: 1; transform: translateY(0); }
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-bottom: 32px;
  }
  .list-cell {
    opacity: 0;
    animation: cellIn var(--dur-normal) var(--ease-out) forwards;
    animation-delay: var(--stagger, 0ms);
  }

  .card-skel { background: var(--bg-card); border: 1px solid var(--border); border-radius: var(--radius-lg); overflow: hidden; }
  .skel-art { aspect-ratio: var(--card-art-aspect); width: 100%; }
  .skel-body { padding: 12px 14px 14px; display: flex; flex-direction: column; gap: 8px; }
  .skel-line { height: 14px; border-radius: var(--radius-sm); }
  .skel-line-lg { width: 70%; }
  .skel-line-sm { width: 50%; height: 10px; }













  .empty { padding: 80px 0; text-align: center; display: flex; flex-direction: column; align-items: center; gap: 8px; color: var(--text-muted); }
  .empty :global(svg) { margin-bottom: 8px; opacity: 0.5; }
  .empty-title { font-size: var(--fs-lg); font-weight: 600; color: var(--text-primary); margin-bottom: 4px; }
  .empty .section-sub { max-width: 480px; }
  .empty-actions { display: inline-flex; gap: 8px; margin-top: 16px; flex-wrap: wrap; justify-content: center; }

  .library-masthead { display: flex; align-items: flex-start; justify-content: space-between; gap: 20px 28px; flex-wrap: wrap; margin-bottom: 24px; }
  .library-heading { min-width: 0; flex: 1 1 260px; }
  .library-summary { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 8px; margin-top: 10px; font-size: 13px; line-height: 1.6; color: var(--text-secondary); }
  .summary-separator { color: var(--text-muted); }
  .library-masthead .header-actions { gap: 8px; }
  .library-workbar { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 12px 20px; padding-bottom: 16px; margin-bottom: 24px; border-bottom: 1px solid var(--border); }
  .library-quick-filters { display: flex; align-items: center; gap: 16px; flex-wrap: wrap; }
  .library-quick-filters button { display: flex; align-items: center; gap: 7px; padding: 10px 0; border-bottom: 2px solid transparent; color: var(--text-secondary); font-size: 14px; }
  .library-quick-filters button.active { color: var(--text-primary); border-bottom-color: var(--text-primary); font-weight: 600; }
  .library-quick-filters button > span { font-size: 12px; color: var(--text-muted); font-variant-numeric: tabular-nums; }
  .library-view-controls { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; margin-left: auto; }
  .library-filter-toggle { display: flex; align-items: center; gap: 8px; height: 38px; padding: 0 12px; border: 1px solid var(--border); border-radius: 8px; font-size: 13px; color: var(--text-secondary); }
  .library-filter-toggle:hover, .library-filter-toggle[aria-expanded="true"] { background: var(--bg-elevated); color: var(--text-primary); }
  .library-filter-options { display: flex; align-items: center; flex-wrap: wrap; gap: 12px; padding: 16px; margin: -8px 0 24px; background: var(--bg-card); border: 1px solid var(--border); border-radius: 10px; }
  .library-filter-options :global(.filter-menu-trigger) { border: 1px solid var(--border); }
  @container workspace (max-width: 560px) {
    .library-masthead { gap: 18px; }
    .library-masthead .header-actions { width: 100%; }
    .library-view-controls { width: 100%; margin: 0; justify-content: space-between; }
  }

  .other-update-games { margin-top: 28px; border-top: 1px solid var(--border); padding-top: 20px; }
  .other-update-games summary { font-size: 14px; cursor: pointer; color: var(--text-secondary); }
  .other-update-games p { font-size: 13px; line-height: 1.5; color: var(--text-muted); margin: 12px 0 20px; }
</style>
