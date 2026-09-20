<script lang="ts" module>
  /** Authoritative component readers for the game detail surface.
   *
   *  Rust owns the observed version, the candidate, the update decision and the applicability of
   *  every component. Nothing here compares versions, resolves a target version or decides
   *  compatibility, trust or install success. A file with no published component reads as
   *  `no-target`: unknown, never offered and never drawn as up to date. */
  import type { ComponentState, GameSnapshot } from "../generated/bindings";
  import type { DllRecord } from "../lib/api";
  import { translate } from "../lib/i18n/index";

  /** Presentation vocabulary shared with `DrawerFeatureList`. It is a projection of the published
   *  `ComponentStatus`, not an independent classification. */
  export type Relation = "outdated" | "same" | "ahead" | "no-target";

  export type ComponentIndex = {
    /** Component by absolute path, the join Rust's `install_dir` + `relative_path` produces. */
    byPath: Map<string, ComponentState>;
    /** Component by `family|filename`; `null` marks a key more than one component claims. */
    byFile: Map<string, ComponentState | null>;
  };

  /** Comparable form of a Windows path. Rust publishes `relative_path` with `/` separators. */
  export function pathKey(path: string): string {
    return path.replace(/[\\/]+/g, "\\").replace(/\\+$/, "").toLowerCase();
  }

  /** Index of the components Rust published for one game, or `null` while it published none for
   *  it. An empty component list is a published fact and returns an empty index. */
  export function componentIndex(snapshots: GameSnapshot[], gameId: string): ComponentIndex | null {
    const snapshot = snapshots.find((candidate) => candidate.id === gameId);
    const components = snapshot?.components;
    if (!snapshot || !components) return null;
    const byPath = new Map<string, ComponentState>();
    const byFile = new Map<string, ComponentState | null>();
    for (const component of components) {
      const relative = component.identity.relative_path.replace(/^[\\/]+/, "");
      byPath.set(pathKey(`${snapshot.install_dir}\\${relative}`), component);
      const fileKey = `${component.identity.family}|${component.identity.filename}`.toLowerCase();
      byFile.set(fileKey, byFile.has(fileKey) ? null : component);
    }
    return { byPath, byFile };
  }

  /** The component Rust published for one scanned file, or `null`. The absolute path is the join;
   *  the family/filename lookup only answers when exactly one component claims that pair, so a
   *  different install root can never silently bind the wrong file. */
  export function componentForRecord(
    index: ComponentIndex | null,
    record: DllRecord,
  ): ComponentState | null {
    if (!index) return null;
    const direct = index.byPath.get(pathKey(record.path));
    if (direct) return direct;
    const filename = record.path.split(/[\\/]/).pop() ?? record.path;
    return index.byFile.get(`${record.family}|${filename}`.toLowerCase()) ?? null;
  }

  /** Relation presented for one file, read from its published component. `no-target` covers every
   *  state that must not be offered — unchecked, unknown, disabled, externally managed,
   *  incompatible, not applicable, and an available update with no published candidate. */
  export function relationOf(component: ComponentState | null): Relation {
    if (!component) return "no-target";
    switch (component.status) {
      case "update_available":
        return component.applicability !== "not_applicable" && component.candidate !== null
          ? "outdated"
          : "no-target";
      case "newer":
        return "ahead";
      case "current":
        return "same";
      default:
        return "no-target";
    }
  }

  /** Version Rust would install for one component. The view never derives it. */
  export function candidateVersion(component: ComponentState | null): string | null {
    return component?.candidate?.package_version ?? null;
  }


  export type BucketStatus = {
    key: string;
    fallbackKey: string;
    tone: "update" | "success" | "info" | "neutral";
  };

  /** Status of one feature bucket, from the components Rust published for its files. Partial and
   *  unknown observations keep their own wording: they are never folded into "Up to date" and
   *  never into "Update ready". */
  export function bucketStatus(
    components: (ComponentState | null)[],
    multiple: boolean,
  ): BucketStatus {
    if (components.some((component) => relationOf(component) === "outdated")) {
      return {
        key: multiple
          ? "component.gameDrawer.status.updatesReady"
          : "component.gameDrawer.status.updateReady",
        fallbackKey: "status.outdated",
        tone: "update",
      };
    }
    const statuses = components.map((component) => component?.status ?? null);
    if (statuses.includes("newer")) {
      return {
        key: "component.gameDrawer.status.aheadOfCatalog",
        fallbackKey: "status.unknown",
        tone: "info",
      };
    }
    if (statuses.includes("externally_managed")) {
      return {
        key: "component.gameDrawer.status.externallyManaged",
        fallbackKey: "status.unknown",
        tone: "neutral",
      };
    }
    if (statuses.includes("incompatible")) {
      return {
        key: "component.gameDrawer.status.incompatible",
        fallbackKey: "status.unknown",
        tone: "neutral",
      };
    }
    if (statuses.includes("disabled")) {
      return {
        key: "component.gameDrawer.status.disabled",
        fallbackKey: "status.unknown",
        tone: "neutral",
      };
    }
    if (statuses.length > 0 && statuses.every((status) => status === "current")) {
      return { key: "status.up_to_date", fallbackKey: "status.up_to_date", tone: "success" };
    }
    if (statuses.length > 0 && statuses.every((status) => status === "unchecked")) {
      return {
        key: "component.gameDrawer.status.notInCatalog",
        fallbackKey: "component.gameDrawer.status.notInCatalog",
        tone: "neutral",
      };
    }
    return { key: "status.unknown", fallbackKey: "status.unknown", tone: "neutral" };
  }
</script>

<script lang="ts">
  import RecipePanel from "./RecipePanel.svelte";
  import { recipeApi } from "../lib/api";
  import { onDestroy } from "svelte";
  import { get } from "svelte/store";
  import { t, locale } from "../lib/i18n/index";
  import { setActiveArt, clearActiveArt } from "../lib/artContext";
  import { coverAccent } from "../lib/coverAccent";
  import { EXTERNAL_URLS } from "../lib/ux";
  import {
    games,
    gameDlls,
    gameDlssEnabler,
    gameDllsLoading,
    gameDllErrors,
    settings,
    persistSettings,
    showToast,
    optimisticToggle,
    rescanGame,
    driverReports,
    hardwarePreference,
    ensureSystemInfo,
    fsr4Capable,
  } from "../lib/stores";
  import { preferredFamily, defaultUpdateFamily } from "../lib/hardwarePreference";
  import { recordUpdatable, isStreamlinePlugin } from "../lib/relation";
  import { authoritativeState } from "../lib/stateSync";
  import { addBlacklistEntry, removeBlacklistEntry, findGameExecutable, detectAnticheat, saveSettings, openPath, readDlssOverrideConfig, type AppSettings, type AntiCheatReport } from "../lib/api";
  import { hasAntiCheat, statusNote, warningMessage, severity, detectedNames } from "../lib/anticheat";
  import { dispatchApply, dispatchStreamlineSet, dispatchDllSet, type ApplyTarget } from "../lib/applyController";
  import {
    familyLabel,
    familyShort,
    recordFeature,
    featureTitle,
    featureIconId,
    featureVendor,
    FEATURE_ORDER,
    VENDOR_ACCENTS,
    DLL_SET_FAMILIES,
    DLL_SET_LABELS,
    FSR4_GATED_FAMILIES,
    filenameFromPath,
    gameOperationLabel,
    type FeatureSlot,
    type DllSetKey,
  } from "../lib/labels";
  import { bestArtSrc } from "../lib/gameArt";
  import ContextMenu, { type ContextMenuAction } from "./ContextMenu.svelte";
  import { focusTrap } from "../actions/focusTrap";
  import DrawerHero from "./DrawerHero.svelte";
  import DrawerFeatureList, { type DrawerFeatureBucket, type DrawerAdvancedRow } from "./DrawerFeatureList.svelte";
  import DrawerFooter, { type DrawerDllSet } from "./DrawerFooter.svelte";

  let { gameId, onClose, onApplyStart }: {
    gameId: string;
    onClose: () => void;
    onApplyStart: () => void;
  } = $props();

  let game = $derived($games.find((g) => g.id === gameId));
  let coverAccentColor = $state<string | null>(null);
  $effect(() => {
    const cover = game ? bestArtSrc(game) : null;
    if (cover) setActiveArt(cover);
  });
  $effect(() => {
    const url = game ? bestArtSrc(game) : null;
    coverAccentColor = null;
    if (!url) return;
    let active = true;
    void coverAccent(url).then((color) => {
      if (active) coverAccentColor = color;
    });
    return () => {
      active = false;
    };
  });

  onDestroy(() => {
    clearActiveArt();
  });

  let records: DllRecord[] = $derived($gameDlls[gameId] ?? []);
  // Components Rust published for this game, or `null` while it published none. Every update
  // decision and every target version on this surface is read from here.
  let components = $derived(componentIndex($authoritativeState.games, gameId));
  let dlssEnabler = $derived($gameDlssEnabler[gameId] ?? false);
  let loading = $derived($gameDllsLoading[gameId] ?? false);
  let scanError = $derived($gameDllErrors[gameId] ?? null);
  let rescanning = $state(false);

  async function doRescan(): Promise<void> {
    if (rescanning) return;
    rescanning = true;
    try {
      await rescanGame(gameId);
      if (!scanError) showToast("success", translate(get(locale), "component.gameDrawer.toast.rescanComplete"));
      else showToast("danger", translate(get(locale), "component.gameDrawer.toast.rescanFailed", { error: scanError }));
    } finally {
      rescanning = false;
    }
  }

  let pref = $derived($settings?.game_preferences[gameId]);
  let disabledFamilies: string[] = $derived(pref?.disabled_families ?? []);
  let pinnedVersions: Record<string, string> = $derived(pref?.pinned_versions ?? {});

  let selected = $state<Record<string, boolean>>({});
  let selectionTouched = $state(false);
  let selectionSignature = $state("");
  let activeGameId = $state<string | null>(null);
  let pickerOpenFor = $state<string | null>(null);
  let expandedFeatures = $state<Record<string, boolean>>({});
  let advancedExpanded = $state(false);

  let dlssExpanded = $state(false);
  let gameExe = $state<string | null>(null);
  let exeResolving = $state(false);
  let exeResolved = $state(false);

  let nvidiaPacked = $derived.by(
    () => $driverReports.find((r) => r.device.vendor === "nvidia")?.installed.packed ?? 0,
  );

  async function resolveExe(): Promise<void> {
    if (!game || exeResolved) return;
    exeResolving = true;
    try {
      gameExe = await findGameExecutable(game.install_dir);
    } catch {
      gameExe = null;
    } finally {
      exeResolving = false;
      exeResolved = true;
    }
  }

  function toggleDlss(): void {
    dlssExpanded = !dlssExpanded;
    if (dlssExpanded) void resolveExe();
  }

  $effect(() => {
    if (gameId !== activeGameId) {
      activeGameId = gameId;
      selectionTouched = false;
      selectionSignature = "";
      const next: Record<string, boolean> = {};
      for (const r of records) {
        const key = rowKey(r);
        next[key] = isOutdated(r) && !disabledFamilies.includes(r.family) && defaultUpdateFamily(r.family, $hardwarePreference);
      }
      selected = next;
      expandedFeatures = {};
      dlssExpanded = false;
      gameExe = null;
      exeResolved = false;
      acReport = null;
      void loadAntiCheat();
      void detectManagedExternally();
      void ensureSystemInfo().catch(() => undefined);
    }
  });

  function rowKey(r: DllRecord): string {
    return `${r.family}|${r.path}`;
  }

  /** The component Rust published for one scanned file, or `null` when it published none. */
  function componentFor(r: DllRecord): ComponentState | null {
    return componentForRecord(components, r);
  }

  function isOutdated(r: DllRecord): boolean {
    if (!recordUpdatable(r, $settings?.update_prefs ?? null)) return false;
    return relation(r) === "outdated";
  }

  /** Rust resolves user pins and publishes the candidate after settings are committed. */
  function targetFor(r: DllRecord): string | null {
    return candidateVersion(componentFor(r));
  }

  /** Candidate Rust published for this file, shown next to the observed version. */
  function latestFor(r: DllRecord): string | null {
    return candidateVersion(componentFor(r));
  }

  /** Presentation relation, projected from the published component status. */
  function relation(r: DllRecord): Relation {
    return relationOf(componentFor(r));
  }

  async function toggleFeatureDisabled(recs: DllRecord[]): Promise<void> {
    if (!$settings) return;
    const before: AppSettings = $settings;
    const families: Set<string> = new Set(recs.map((r) => r.family));
    const wasAllDisabled = [...families].every((f) => disabledFamilies.includes(f));
    const prefs = { ...before.game_preferences };
    const cur = prefs[gameId] ?? { disabled_families: [], pinned_versions: {} };
    let next = [...cur.disabled_families];
    if (wasAllDisabled) {
      next = next.filter((f) => !families.has(f));
    } else {
      for (const f of families) if (!next.includes(f)) next.push(f);
    }
    prefs[gameId] = { ...cur, disabled_families: next };
    const after: AppSettings = { ...before, game_preferences: prefs };
    const loc = get(locale);
    const featureName = recs.length > 0 ? familyShort(recs[0].family) : translate(loc, "component.gameDrawer.featureFallback");
    await optimisticToggle({
      applyOptimistic: () => settings.set(after),
      revert: () => settings.set(before),
      commit: () => saveSettings(after),
      message: translate(loc, wasAllDisabled ? "component.gameDrawer.toast.featureEnabled" : "component.gameDrawer.toast.featureDisabled", {
        feature: featureName,
        game: game?.name ?? translate(loc, "component.gameDrawer.thisGame"),
      }),
    });
  }

  async function setPin(key: string, version: string | null): Promise<void> {
    if (!$settings) return;
    const prefs = { ...$settings.game_preferences };
    const cur = prefs[gameId] ?? { disabled_families: [], pinned_versions: {} };
    const pins = { ...cur.pinned_versions };
    if (version === null) {
      delete pins[key];
    } else {
      pins[key] = version;
    }
    prefs[gameId] = { ...cur, pinned_versions: pins };
    await persistSettings({ ...$settings, game_preferences: prefs });
  }

  function pickPrimary(feature: FeatureSlot, recs: DllRecord[]): DllRecord {
    if (feature === "dlss_sr") {
      const r = recs.find((x) => x.family === "dlss_sr") ?? recs.find((x) => x.family === "streamline" && filenameFromPath(x.path).toLowerCase().endsWith("sl.dlss.dll"));
      if (r) return r;
    }
    if (feature === "dlss_fg") {
      const r = recs.find((x) => x.family === "dlss_fg") ?? recs.find((x) => x.family === "streamline" && filenameFromPath(x.path).toLowerCase().endsWith("sl.dlss_g.dll"));
      if (r) return r;
    }
    if (feature === "dlss_rr") {
      const r = recs.find((x) => x.family === "dlss_rr") ?? recs.find((x) => x.family === "streamline" && filenameFromPath(x.path).toLowerCase().endsWith("sl.dlss_d.dll"));
      if (r) return r;
    }
    if (feature === "fsr_upscaler") {
      const r = recs.find((x) => x.family === "fsr_upscaler") ?? recs.find((x) => x.family === "fsr_upscaler_vk") ?? recs.find((x) => x.family === "fsr_loader");
      if (r) return r;
    }
    if (feature === "xess_sr") {
      const r = recs.find((x) => x.family === "xess_sr") ?? recs.find((x) => x.family === "xess_sr_dx11") ?? recs.find((x) => x.family === "xell");
      if (r) return r;
    }
    return recs[0];
  }

  let featureBuckets = $derived.by<DrawerFeatureBucket[]>(() => {
    const map = new Map<FeatureSlot, DllRecord[]>();
    for (const r of records) {
      const f = recordFeature(r);
      if (!map.has(f)) map.set(f, []);
      map.get(f)!.push(r);
    }
    const out: DrawerFeatureBucket[] = [];
    for (const fid of FEATURE_ORDER) {
      const recs = map.get(fid);
      if (!recs || recs.length === 0) continue;
      const primary = pickPrimary(fid, recs);
      // Every flag below reads the components Rust published for these files.
      const states = recs.map((r) => componentFor(r));
      const anyOutdated = recs.some((r) => relation(r) === "outdated");
      const anyAhead = recs.some((r) => relation(r) === "ahead");
      const inCatalog = recs.filter((r) => relation(r) !== "no-target");
      const allUpToDate = inCatalog.length > 0 && inCatalog.every((r) => relation(r) === "same");
      const allDisabled = recs.every((r) => disabledFamilies.includes(r.family));
      let label: string;
      let tone: DrawerFeatureBucket["statusTone"];
      if (allDisabled) {
        // A family the user switched off for this game. A local preference, not a published state.
        label = $t("component.gameDrawer.status.disabled");
        tone = "neutral";
      } else {
        const status = bucketStatus(states, recs.length > 1);
        label = translate($locale, status.key);
        tone = status.tone;
      }
      out.push({
        feature: fid,
        records: recs,
        primary,
        title: featureTitle(fid),
        blurb: $t("feature." + fid + ".blurb"),
        iconId: featureIconId(fid),
        accent: VENDOR_ACCENTS[featureVendor(fid)] ?? "#94a3b8",
        anyOutdated,
        anyAhead,
        allUpToDate,
        allDisabled,
        statusLabel: label,
        statusTone: tone,
      });
    }
    return out;
  });

  let advancedRecords = $derived(records.filter((r) => recordFeature(r) === "advanced"));

  let advancedRows = $derived.by<DrawerAdvancedRow[]>(() => {
    const map = new Map<string, DllRecord[]>();
    for (const r of advancedRecords) {
      const key = r.family === "streamline" ? `streamline:${filenameFromPath(r.path).toLowerCase()}` : r.family;
      if (!map.has(key)) map.set(key, []);
      map.get(key)!.push(r);
    }
    const out: DrawerAdvancedRow[] = [];
    for (const [key, recs] of map) {
      const fname = filenameFromPath(recs[0].path);
      const label = key.startsWith("streamline:")
        ? `Streamline · ${fname.replace(/\.dll$/i, "").replace(/^sl\./i, "").replace(/_/g, " ")}`
        : familyLabel(recs[0].family);
      out.push({ family: key, label, records: recs, primary: recs[0], anyOutdated: recs.some((r) => relation(r) === "outdated") });
    }
    return out.sort((a, b) => a.label.localeCompare(b.label));
  });

  let preferredFeatureBuckets = $derived(featureBuckets.filter(bucket => bucket.records.some(record => preferredFamily(record.family, $hardwarePreference))));
  let otherFeatureBuckets = $derived(featureBuckets.filter(bucket => !bucket.records.some(record => preferredFamily(record.family, $hardwarePreference))));
  let recommendedCount = $derived(records.filter(record => isOutdated(record) && !disabledFamilies.includes(record.family) && preferredFamily(record.family, $hardwarePreference)).length);

  $effect(() => {
    const preference = $hardwarePreference;
    const signature = `${gameId}:${[...preference.vendors].sort().join(",")}:${records.map(record => `${rowKey(record)}:${isOutdated(record)}:${disabledFamilies.includes(record.family)}`).join("|")}`;
    if (selectionTouched || signature === selectionSignature || activeGameId !== gameId) return;
    selectionSignature = signature;
    const next: Record<string, boolean> = {};
    for (const record of records) next[rowKey(record)] = isOutdated(record) && !disabledFamilies.includes(record.family) && defaultUpdateFamily(record.family, preference);
    selected = next;
    // A supporting library selected by default must not be hidden from the user.
    advancedExpanded = advancedRows.some(row => row.records.some(record => next[rowKey(record)]));
  });

  function selectAllOutdated(): void {
    selectionTouched = true;
    const next: Record<string, boolean> = { ...selected };
    for (const r of records) {
      if (isOutdated(r) && !disabledFamilies.includes(r.family) && preferredFamily(r.family, $hardwarePreference)) {
        next[rowKey(r)] = true;
      }
    }
    selected = next;
  }

  function clearSelection(): void {
    selectionTouched = true;
    selected = {};
  }

  function setFileSelection(key: string, checked: boolean): void {
    selectionTouched = true;
    selected = { ...selected, [key]: checked };
  }

  function toggleFeatureSelection(bucket: DrawerFeatureBucket, checked: boolean): void {
    selectionTouched = true;
    const next = { ...selected };
    for (const r of bucket.records) {
      if (disabledFamilies.includes(r.family)) continue;
      const rel = relation(r);
      if (rel === "same" || rel === "no-target") continue;
      next[rowKey(r)] = checked;
    }
    selected = next;
  }

  function featureSelectionState(bucket: DrawerFeatureBucket): "all" | "some" | "none" {
    const eligible = bucket.records.filter((r) => !disabledFamilies.includes(r.family) && relation(r) !== "same" && relation(r) !== "no-target");
    if (eligible.length === 0) return "none";
    const sel = eligible.filter((r) => selected[rowKey(r)]).length;
    if (sel === 0) return "none";
    if (sel === eligible.length) return "all";
    return "some";
  }

  function toggleFeatureExpanded(feature: string): void {
    expandedFeatures = { ...expandedFeatures, [feature]: !expandedFeatures[feature] };
  }

  function selectedRecords(): { record: DllRecord; target: string }[] {
    const out: { record: DllRecord; target: string }[] = [];
    for (const r of records) {
      if (!selected[rowKey(r)]) continue;
      if (disabledFamilies.includes(r.family)) continue;
      const published = componentFor(r);
      if (!published || published.applicability === "not_applicable" || ["disabled", "externally_managed", "incompatible", "unknown", "unchecked"].includes(published.status)) continue;
      const tgt = targetFor(r);
      if (!tgt) continue;
      out.push({ record: r, target: tgt });
    }
    return out;
  }

  let acConfirming = $state(false);

  $effect(() => {
    if (gameId) acConfirming = false;
  });

  function requestApply(): void {
    if (busy || selectedCount === 0) return;
    if (acActive && acSeverity === "danger" && !acConfirming) {
      acConfirming = true;
      return;
    }
    acConfirming = false;
    void applySelected();
  }

  function cancelApplyConfirm(): void {
    acConfirming = false;
  }

  async function applySelected(): Promise<void> {
    if (!game) return;
    const items = selectedRecords();
    if (items.length === 0) {
      showToast("warning", translate(get(locale), "component.gameDrawer.toast.nothingSelected"));
      return;
    }
    const game_label = gameOperationLabel(game.launcher, game.name);
    const targets: ApplyTarget[] = items.map((it) => ({
      game_id: game!.id,
      game_label,
      record: it.record,
      target_version: it.target,
      catalog_family: componentFor(it.record)?.candidate?.family,
    }));
    await dispatchApply(targets, { showModal: onApplyStart });
    try {
      await rescanGame(game.id);
      selected = {};
    } catch (err: unknown) {
      showToast(
        "warning",
        translate(get(locale), "component.gameDrawer.toast.rescanAfterApplyFailed", { error: String(err) }),
      );
    }
  }

  let streamlineSetMembers = $derived(
    records.filter((r) => isStreamlinePlugin(filenameFromPath(r.path)) && isOutdated(r)),
  );
  let streamlineSetTarget = $derived(
    streamlineSetMembers.length ? targetFor(streamlineSetMembers[0]) : null,
  );

  function dllSetMembers(key: DllSetKey): DllRecord[] {
    return records.filter((r) => DLL_SET_FAMILIES[key].includes(r.family) && isOutdated(r));
  }

  function versionMajor(version: string | null): number {
    const major = Number.parseInt(version?.split(".")[0] ?? "", 10);
    return Number.isNaN(major) ? 0 : major;
  }

  let fsrSetMembers = $derived(dllSetMembers("fsr"));
  let xessSetMembers = $derived(dllSetMembers("xess"));
  let fsrSetNeedsFsr4 = $derived(
    fsrSetMembers.some(
      (r) => FSR4_GATED_FAMILIES.includes(r.family) && versionMajor(targetFor(r)) >= 4,
    ),
  );
  let fsrSetBlocked = $derived(fsrSetNeedsFsr4 && !$fsr4Capable);

  let dllSets = $derived.by<DrawerDllSet[]>(() => {
    const out: DrawerDllSet[] = [];
    if (fsrSetMembers.length >= 2) {
      out.push({
        key: "fsr",
        label: DLL_SET_LABELS.fsr,
        count: fsrSetMembers.length,
        target: targetFor(fsrSetMembers[0]),
        blocked: fsrSetBlocked,
      });
    }
    if (xessSetMembers.length >= 2) {
      out.push({
        key: "xess",
        label: DLL_SET_LABELS.xess,
        count: xessSetMembers.length,
        target: targetFor(xessSetMembers[0]),
        blocked: false,
      });
    }
    return out;
  });

  async function applyDllSetAction(key: DllSetKey): Promise<void> {
    if (!game) return;
    const members = dllSetMembers(key);
    if (members.length === 0) return;
    const game_label = gameOperationLabel(game.launcher, game.name);
    const targets: ApplyTarget[] = [];
    for (const r of members) {
      const tgt = targetFor(r);
      if (!tgt) continue;
      targets.push({ game_id: game.id, game_label, record: r, target_version: tgt, catalog_family: componentFor(r)?.candidate?.family });
    }
    await dispatchDllSet(targets, DLL_SET_LABELS[key], { showModal: onApplyStart });
    try {
      await rescanGame(game.id);
      selected = {};
    } catch (err: unknown) {
      showToast(
        "warning",
        translate(get(locale), "component.gameDrawer.toast.rescanAfterApplyFailed", { error: String(err) }),
      );
    }
  }

  let managedExternally = $state(false);
  async function detectManagedExternally(): Promise<void> {
    const id = gameId;
    managedExternally = false;
    await resolveExe();
    if (gameId !== id || !gameExe) return;
    try {
      const readback = await readDlssOverrideConfig({ scope: "per_game", executable_path: gameExe });
      if (gameId !== id) return;
      managedExternally =
        readback.source === "per_game" &&
        readback.active_count > 0 &&
        !$settings?.game_preferences[id];
    } catch {
      if (gameId === id) managedExternally = false;
    }
  }

  async function applyStreamlineSetAction(): Promise<void> {
    if (!game || streamlineSetMembers.length === 0) return;
    const game_label = gameOperationLabel(game.launcher, game.name);
    const targets: ApplyTarget[] = [];
    for (const r of streamlineSetMembers) {
      const tgt = targetFor(r);
      if (!tgt) continue;
      targets.push({ game_id: game.id, game_label, record: r, target_version: tgt, catalog_family: componentFor(r)?.candidate?.family });
    }
    await dispatchStreamlineSet(targets, { showModal: onApplyStart });
    try {
      await rescanGame(game.id);
      selected = {};
    } catch (err: unknown) {
      showToast(
        "warning",
        translate(get(locale), "component.gameDrawer.toast.rescanAfterApplyFailed", { error: String(err) }),
      );
    }
  }

  let rowMenu = $state<{ x: number; y: number; primaryKey: string } | null>(null);
  let rowMenuItems = $derived([
    { action: "open_folder" as ContextMenuAction, label: $t("view.library.menu.openFolder") },
    { action: "scan" as ContextMenuAction, label: $t("component.gameDrawer.menu.rescan") },
    { action: "pin" as ContextMenuAction, label: $t("view.library.menu.pin") },
    { action: "hide" as ContextMenuAction, label: $t("component.gameDrawer.menu.hideGame") },
  ]);

  function openRowMenu(primaryKey: string, e: MouseEvent): void {
    e.preventDefault();
    rowMenu = { x: e.clientX, y: e.clientY, primaryKey };
  }

  function openRowMenuAt(primaryKey: string, x: number, y: number): void {
    rowMenu = { x, y, primaryKey };
  }

  async function onRowMenuSelect(action: ContextMenuAction): Promise<void> {
    const key = rowMenu?.primaryKey ?? null;
    switch (action) {
      case "open_folder":
        await openFolder();
        break;
      case "scan":
        await doRescan();
        break;
      case "pin":
        if (key) pickerOpenFor = key;
        break;
      case "hide":
        await toggleHidden();
        break;
    }
  }

  async function openFolder(): Promise<void> {
    if (!game) return;
    try {
      await openPath(game.install_dir);
    } catch (err: unknown) {
      showToast("danger", translate(get(locale), "component.gameDrawer.toast.openFolderFailed", { error: String(err) }));
    }
  }

  async function openAnticheatLink(): Promise<void> {
    try {
      const { open } = await import("@tauri-apps/plugin-shell");
      await open(acLearnUrl);
    } catch (err) {
      showToast("warning", translate(get(locale), "component.gameDrawer.toast.openLinkFailed", { error: String(err) }));
    }
  }

  let isHidden = $derived(($settings?.blacklist ?? []).includes(gameId));

  async function toggleHidden(): Promise<void> {
    if (!game) return;
    const wasHidden = isHidden;
    const loc = get(locale);
    try {
      const next = wasHidden ? await removeBlacklistEntry(game.id) : await addBlacklistEntry(game.id);
      if ($settings) settings.set({ ...$settings, blacklist: next });
      showToast(
        wasHidden ? "success" : "info",
        translate(loc, wasHidden ? "view.library.toast.gameRestored" : "view.library.toast.gameHidden", { name: game.name }),
      );
      if (!wasHidden) onClose();
    } catch (err: unknown) {
      showToast("danger", translate(loc, wasHidden ? "view.library.toast.restoreFailed" : "view.library.toast.hideFailed", { error: String(err) }));
    }
  }

  let outdatedCount = $derived(
    records.filter(isOutdated).filter((r) => !disabledFamilies.includes(r.family)).length,
  );
  let selectedCount = $derived(Object.values(selected).filter(Boolean).length);
  let aheadCount = $derived(
    records.filter((r) => selected[rowKey(r)] && relation(r) === "ahead").length,
  );
  let acReport = $state<AntiCheatReport | null>(null);
  async function loadAntiCheat(): Promise<void> {
    if (!game) return;
    try {
      acReport = await detectAnticheat(game.install_dir, game.app_id, game.name);
    } catch {
      acReport = null;
    }
  }

  let acActive = $derived(hasAntiCheat(acReport));
  let acStatus = $derived(acReport ? statusNote(acReport) : null);
  let acSeverity = $derived(acReport ? severity(acReport) : "warning");
  let acNames = $derived(acReport ? detectedNames(acReport) : "");
  let acLearnUrl = $derived(acReport?.source_url ?? EXTERNAL_URLS.anticheatFaq);
  let acWarningMessage = $derived(acReport ? warningMessage(acReport) : "");

  let detailTab = $state<"updates" | "advanced">("updates");
  let busy = $derived(loading || rescanning);
</script>

<svelte:window onkeydown={(e) => { if (game && e.key === "Escape" && !e.defaultPrevented) onClose(); }} />

{#if game}
  <div class="detail-view" role="dialog" aria-modal="true" aria-label={game.name} tabindex="-1" use:focusTrap={{ initialFocusRing: false }}>
    <DrawerHero
      {game}
      {coverAccentColor}
      {loading}
      {rescanning}
      {scanError}
      recordCount={records.length}
      outdatedCount={$hardwarePreference.known ? recommendedCount : outdatedCount}
      {aheadCount}
      {acActive}
      {acSeverity}
      {acStatus}
      {acWarningMessage}
      {dlssEnabler}
      {managedExternally}
      {onClose}
      onLearnMore={() => void openAnticheatLink()}
    />

    <nav class="detail-tabs" aria-label={game.name}>
      <button class:active={detailTab === "updates"} aria-pressed={detailTab === "updates"} onclick={() => (detailTab = "updates")}>{$t("component.detail.updates")}</button>
      <button class:active={detailTab === "advanced"} aria-pressed={detailTab === "advanced"} onclick={() => (detailTab = "advanced")}>{$t("component.detail.advanced")}</button>
    </nav>
    <div class="drawer-body">
      {#if detailTab === "updates"}
      {#if busy}
        <div class="loading-state scanning" role="status" aria-live="polite">
          <div class="scan-head">
            <span class="spinner spin"></span>
            <span class="scan-label">{rescanning ? $t("component.gameDrawer.loading.rescanning") : $t("component.gameDrawer.loading.scanning")} — {game.install_dir.split(/[\\/]/).pop()}</span>
          </div>
          <div class="scan-skeleton" aria-hidden="true">
            {#each [70, 56, 64, 48] as w, i (i)}
              <div class="scan-skel-row">
                <div class="scan-skel-icon skeleton"></div>
                <div class="scan-skel-lines">
                  <div class="scan-skel-line skeleton" style:width="{w}%"></div>
                  <div class="scan-skel-line sm skeleton"></div>
                </div>
                <div class="scan-skel-pill skeleton"></div>
              </div>
            {/each}
          </div>
        </div>
      {:else}
        {#if scanError}
          <div class="empty-state error-state">
            <svg width="34" height="34" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>
            <h3>{$t("component.gameDrawer.scanError.title")}</h3>
            <p class="error-msg mono">{scanError}</p>
            <p class="error-hint">{$t("component.gameDrawer.scanError.hint")}</p>
            <button class="btn btn-primary" onclick={doRescan}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/></svg>
              {$t("component.gameDrawer.scanError.retry")}
            </button>
          </div>
        {:else if records.length === 0}
          <div class="empty-state">
            <svg width="34" height="34" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
            <h3>{$t("component.gameDrawer.empty.title")}</h3>
            <p>{$t("component.gameDrawer.empty.body")}</p>
            <div class="empty-actions">
              <button class="btn btn-accent" onclick={doRescan}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/></svg>
                {$t("view.library.rescan")}
              </button>
              <button class="btn btn-ghost" onclick={openFolder}>{$t("view.library.menu.openFolder")}</button>
            </div>
          </div>
        {/if}
        <DrawerFeatureList
          hasRecords={!scanError && records.length > 0}
          recordCount={records.length}
          {outdatedCount}
          {selectedCount}
          featureBuckets={preferredFeatureBuckets}
          {otherFeatureBuckets}
          {recommendedCount}
          hardwareKnown={$hardwarePreference.known}
          {advancedRows}
          {selected}
          {disabledFamilies}
          {pinnedVersions}
          {expandedFeatures}
          {advancedExpanded}
          {pickerOpenFor}
          {dlssExpanded}
          dlssExe={gameExe}
          dlssExeResolving={exeResolving}
          dlssDriverPacked={nvidiaPacked}
          {rowKey}
          {relation}
          {targetFor}
          {latestFor}
          {featureSelectionState}
          onSelectAllOutdated={selectAllOutdated}
          onClearSelection={clearSelection}
          onToggleFeatureSelection={toggleFeatureSelection}
          onToggleFileSelection={setFileSelection}
          onToggleFeatureDisabled={(recs) => void toggleFeatureDisabled(recs)}
          onSetPin={(key, version) => void setPin(key, version)}
          onSetPickerOpen={(key) => (pickerOpenFor = key)}
          onToggleFeatureExpanded={toggleFeatureExpanded}
          onToggleAdvanced={() => (advancedExpanded = !advancedExpanded)}
          onToggleDlss={toggleDlss}
          onRowContextMenu={openRowMenu}
          onRowMenuAnchor={openRowMenuAt}
        />
      {/if}
      {:else}
        <section class="detail-location">
          <h3>{$t("component.detail.installLocation")}</h3>
          <p class="mono">{game.install_dir}</p>
        </section>
        <section class="local-packages">
          <h3>{$t("component.detail.localPackages")}</h3>
          <p>{$t("component.detail.localPackagesHelp")}</p>
          {#key game.id}<RecipePanel gameId={game.id} api={recipeApi} />{/key}
        </section>
      {/if}
    </div>

    <DrawerFooter
      {selectedCount}
      {aheadCount}
      {isHidden}
      {busy}
      streamlineSetCount={streamlineSetMembers.length}
      {streamlineSetTarget}
      {dllSets}
      {acActive}
      {acSeverity}
      {acNames}
      {acConfirming}
      onOpenFolder={() => void openFolder()}
      onRescan={() => void doRescan()}
      onToggleHidden={() => void toggleHidden()}
      onApplyStreamlineSet={() => void applyStreamlineSetAction()}
      onApplyDllSet={(key) => void applyDllSetAction(key)}
      onRequestApply={requestApply}
      onCancelApplyConfirm={cancelApplyConfirm}
    />
  </div>
  {#if rowMenu}
    <ContextMenu
      x={rowMenu.x}
      y={rowMenu.y}
      items={rowMenuItems}
      onSelect={(a) => void onRowMenuSelect(a)}
      onClose={() => (rowMenu = null)}
    />
  {/if}
{/if}

<style>
  .detail-tabs { display: flex; gap: 24px; padding: 0 clamp(16px, 4cqi, 36px); border-bottom: 1px solid var(--border); flex-shrink: 0; }
  .detail-tabs button { padding: 14px 0; background: none; border: 0; border-bottom: 2px solid transparent; color: var(--text-secondary); cursor: pointer; font-weight: 600; }
  .detail-tabs button.active { border-color: var(--text-primary); color: var(--text-primary); }
  .detail-location h3, .local-packages h3 { font-size: 16px; margin-bottom: 8px; }
  .detail-location p { overflow-wrap: anywhere; font-size: 12px; color: var(--text-secondary); }
  .local-packages { padding-top: 20px; border-top: 1px solid var(--border); }
  .local-packages > p { color: var(--text-secondary); font-size: 13px; margin-bottom: 20px; }


  .detail-view {
    --art-chrome-fg: #fff;
    --art-chrome-fg-dim: rgba(255, 255, 255, 0.88);
    --art-chrome-scrim: rgba(0, 0, 0, 0.55);
    --art-chrome-scrim-strong: rgba(0, 0, 0, 0.72);
    --art-chrome-border: rgba(255, 255, 255, 0.3);
    --art-chrome-border-strong: rgba(255, 255, 255, 0.5);
    container-type: inline-size;
    container-name: drawer;
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
    width: 100%;
    min-height: 0;
    gap: 0;
    background: var(--bg-card);
    border: none;
    border-radius: 0;
    overflow: hidden;
  }

  .drawer-body {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: 20px clamp(16px, 4cqi, 36px) 28px;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .loading-state, .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    padding: 48px 20px 36px;
    text-align: center;
    color: var(--text-muted);
  }
  .loading-state.scanning {
    align-items: stretch;
    text-align: left;
    padding: var(--space-2) 0 var(--space-3);
    gap: var(--space-4);
  }
  .scan-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--fs-sm);
    color: var(--text-secondary);
    font-weight: 500;
  }
  .scan-label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .scan-skeleton { display: flex; flex-direction: column; gap: var(--space-2); }
  .scan-skel-row {
    display: grid;
    grid-template-columns: 34px 1fr 56px;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--bg-card);
  }
  .scan-skel-icon { width: 34px; height: 34px; border-radius: var(--radius-md); }
  .scan-skel-lines { display: flex; flex-direction: column; gap: 7px; min-width: 0; }
  .scan-skel-line { height: 11px; border-radius: var(--radius-full); }
  .scan-skel-line.sm { height: 8px; width: 34%; }
  .scan-skel-pill { height: 18px; border-radius: var(--radius-full); }
  .empty-state h3 { color: var(--text-primary); font-size: var(--fs-lg); font-weight: 600; margin-top: 6px; }
  .empty-state p { font-size: var(--fs-sm); max-width: 360px; line-height: 1.55; }
  .empty-state svg { opacity: 0.6; }
  .empty-state .btn { margin-top: var(--space-3); }
  .empty-state .empty-actions { display: flex; gap: var(--space-2); margin-top: var(--space-3); }
  .empty-state.error-state svg { color: var(--danger); opacity: 0.85; }
  .empty-state.error-state h3 { color: var(--danger); }
  .empty-state .error-msg { color: var(--danger); background: var(--danger-dim); padding: var(--space-2) var(--space-3); border-radius: var(--radius-md); font-size: var(--fs-xs); max-width: 100%; overflow-wrap: anywhere; }
  .empty-state .error-hint { color: var(--text-secondary); }

  .spinner {
    width: 14px;
    height: 14px;
  }
</style>
