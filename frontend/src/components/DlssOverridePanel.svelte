<script lang="ts">
  import { onMount } from "svelte";
  import {
    applyDlssOverride,
    resetDlssOverride,
    readDlssOverrideConfig,
    dlssCapabilities,
    openUrl,
    type OverrideScope,
    type DlssCapabilitySnapshot,
    type DlssOverrideSource,
    type FgPreset,
    type FrameGenMode,
    type FrameGenCount,
    type RrPreset,
    type SrPreset,
  } from "../lib/api";
  import {
    SR_PRESET_OPTIONS,
    RR_PRESET_OPTIONS,
    FG_MODE_OPTIONS,
    FG_COUNT_OPTIONS,
    emptyDlssPresetWriteDraft,
    dynamicMfgAvailable,
    dlssWriteGate,
    featurePresetDescription,
    presetEntries,
    presetSettingIds,
    rawValueHex,
    rrLocalDescription,
    presetReadOnlyReasonKey,
    presetWriteDraftFromReadback,
    PRESET_FEATURES,
    writablePresetOptions,
    type DlssPresetWriteDraft,
    type PresetFeature,
  } from "../lib/dlss";
  import { showToast } from "../lib/stores";
  import { t, translate, locale } from "../lib/i18n/index";
  import { get } from "svelte/store";
  import Checkbox from "./Checkbox.svelte";
  import Select from "./Select.svelte";

  let fgModeSelectOptions = $derived<
    { value: FrameGenMode | null; label: string; disabled?: boolean }[]
  >([
    { value: null, label: $t("component.dlss.noModeOverride") },
    ...FG_MODE_OPTIONS.map((o) => ({
      value: o.value,
      label:
        o.value === "dynamic" && !dynamicOk
          ? $t("component.dlss.needsDriver", { label: $t("dlss.fgMode." + o.value + ".label"), version: "595.97" })
          : $t("dlss.fgMode." + o.value + ".label"),
      disabled: o.value === "dynamic" && !dynamicOk,
    })),
  ]);
  let fgCountSelectOptions = $derived<{ value: FrameGenCount | null; label: string }[]>([
    { value: null, label: $t("component.dlss.appControlled") },
    ...FG_COUNT_OPTIONS.filter((o) => o.value !== "app_controlled").map((o) => ({
      value: o.value,
      label: $t("dlss.fgCount." + o.value + ".label"),
    })),
  ]);

  let { scope, driverPacked = 0 }: { scope: OverrideScope; driverPacked?: number } = $props();

  let writeConfig = $state<DlssPresetWriteDraft>(emptyDlssPresetWriteDraft());
  let baseline = $state<DlssPresetWriteDraft>(emptyDlssPresetWriteDraft());
  let readError = $state<string | null>(null);
  const fieldIds: Record<keyof DlssPresetWriteDraft, number> = {
    enable_sr_dll_override: 0x10E41E01, sr_preset: 0x10E41DF3,
    enable_rr_dll_override: 0x10E41E02, rr_preset: 0x10E41DF7,
    enable_fg_dll_override: 0x10E41E03, fg_preset: 0x10E41DF1,
    fg_mode: 0x10308298, fg_fixed_count: 0x104D6667, fg_dynamic_target_fps: 0x10CF4125,
  };
  let changedIds = $derived((Object.keys(fieldIds) as (keyof DlssPresetWriteDraft)[])
    .filter(key => writeConfig[key] !== baseline[key]).map(key => fieldIds[key]));
  let busy = $state(false);
  let activeCount = $state(0);
  let source = $state<DlssOverrideSource>("none");
  let capabilities = $state<DlssCapabilitySnapshot | null>(null);
  let capabilityLoaded = $state(false);
  let referenceOpen = $state(false);

  /* Driver profile persistence is separate from in-game compatibility evidence.
     No snapshot, no adapter and a not-eligible assessment all deny the write; a documented provider
     namespace is never promoted into a support claim, and the reason shown says what is missing
     without asserting compatibility either way. */
  let gate = $derived(dlssWriteGate(capabilities));
  let writable = $derived(gate.eligible);
  let presets = $derived(capabilities?.presets ?? null);
  let controlsDisabled = $derived(busy || !writable || readError !== null);
  function settingAvailable(id: number): boolean {
    return capabilities?.profile_access ? capabilities.profile_access.setting_ids.includes(id) : writable;
  }
  function presetOptionLabel(value: string): string {
    if (value === "latest") return $t("view.catalog.latest");
    if (value === "off" || value === "default") return $t("dlss.preset.default.label");
    if (/^[a-o]$/.test(value)) return $t("dlss.preset." + value + ".label");
    return `Preset ${value.toUpperCase()}`;
  }
  let srSelectOptions = $derived<{ value: SrPreset | null; label: string }[]>([
    { value: null, label: $t("component.dlss.noPresetOverride") },
    ...writablePresetOptions(presets, "sr").map((option) => ({
      value: option.value as SrPreset,
      label: presetOptionLabel(option.value),
    })),
    ...(writeConfig.sr_preset && !writablePresetOptions(presets, "sr").some((option) => option.value === writeConfig.sr_preset)
      ? [{ value: writeConfig.sr_preset, label: presetOptionLabel(writeConfig.sr_preset) }]
      : []),
  ]);
  let rrSelectOptions = $derived<{ value: RrPreset | null; label: string }[]>([
    { value: null, label: $t("component.dlss.noPresetOverride") },
    ...writablePresetOptions(presets, "rr").map((option) => ({
      value: option.value as RrPreset,
      label: presetOptionLabel(option.value),
    })),
    ...(writeConfig.rr_preset && !writablePresetOptions(presets, "rr").some((option) => option.value === writeConfig.rr_preset)
      ? [{ value: writeConfig.rr_preset, label: presetOptionLabel(writeConfig.rr_preset) }]
      : []),
  ]);
  let fgSelectOptions = $derived<{ value: FgPreset | null; label: string }[]>([
    { value: null, label: $t("component.dlss.noPresetOverride") },
    ...writablePresetOptions(presets, "fg").map((option) => ({
      value: option.value as FgPreset,
      label: presetOptionLabel(option.value),
    })),
    ...(writeConfig.fg_preset && !writablePresetOptions(presets, "fg").some((option) => option.value === writeConfig.fg_preset)
      ? [{ value: writeConfig.fg_preset, label: presetOptionLabel(writeConfig.fg_preset) }]
      : []),
  ]);
  let sourceLabel = $derived(
    source === "none"
      ? null
      : source === "per_game"
        ? $t("component.dlss.source.fromDriver")
        : scope.scope === "global"
          ? $t("component.dlss.source.setInDriver")
          : $t("component.dlss.source.inheritedGlobal"),
  );

  let dynamicOk = $derived(driverPacked === 0 || dynamicMfgAvailable(driverPacked));

  let srHelp = $derived(SR_PRESET_OPTIONS.find((o) => o.value === writeConfig.sr_preset) ?? null);
  let rrHelp = $derived(RR_PRESET_OPTIONS.find((o) => o.value === writeConfig.rr_preset) ?? null);

  /* Super Resolution and Ray Reconstruction never share preset copy. Each description comes from
     its own backend registry; when the registry has no entry for the selected identifier, Ray
     Reconstruction falls back only to Ray-Reconstruction-specific local copy and otherwise shows
     no description at all, rather than borrowing the Super Resolution text. */
  let srDescription = $derived(
    featurePresetDescription(presets, "sr", writeConfig.sr_preset) ??
      (writeConfig.sr_preset === null ? null : $t("dlss.preset." + writeConfig.sr_preset + ".desc")),
  );
  let rrDescription = $derived(
    featurePresetDescription(presets, "rr", writeConfig.rr_preset) ??
      rrLocalDescription(writeConfig.rr_preset),
  );
  let fgDescription = $derived(
    featurePresetDescription(presets, "fg", writeConfig.fg_preset),
  );

  let fgModeHelp = $derived(FG_MODE_OPTIONS.find((o) => o.value === writeConfig.fg_mode) ?? null);
  let fgCountHelp = $derived(FG_COUNT_OPTIONS.find((o) => o.value === writeConfig.fg_fixed_count) ?? null);

  const FEATURE_TITLE_KEY: Record<PresetFeature, string> = {
    sr: "component.dlss.superResolution",
    rr: "component.dlss.rayReconstruction",
    fg: "component.dlss.frameGeneration",
    nr: "component.dlss.neuralRendering",
  };

  async function refresh(): Promise<void> {
    try {
      const readback = await readDlssOverrideConfig(scope);
      readError = null;
      writeConfig = presetWriteDraftFromReadback(readback.config);
      for (const feature of ["sr", "rr", "fg"] as const) {
        const value = readback.observations?.find(item => item.setting_id === fieldIds[`${feature}_preset`])?.effective_value;
        if (value?.value_type === "dword") {
          const raw = value.value;
          // Preserve the feature-specific typed identifier, including Latest and FG Default.
          const token = raw === 0x00ffffff ? "latest" : raw === 0x00fffffe && feature === "fg" ? "default"
            : raw === 0 ? "off" : raw >= 1 && raw <= (feature === "fg" ? 26 : 15) ? String.fromCharCode(96 + raw) : null;
          if (feature === "sr") writeConfig.sr_preset = token as SrPreset | null;
          if (feature === "rr") writeConfig.rr_preset = token as RrPreset | null;
          if (feature === "fg") writeConfig.fg_preset = token as FgPreset | null;
        }
      }
      baseline = { ...writeConfig };
      activeCount = readback.active_count;
      source = readback.source;
    } catch (error) {
      readError = error && typeof error === "object" && "message" in error ? String(error.message) : String(error);
    }
  }

  /* Fail closed: any failure to obtain the capability snapshot leaves `capabilities` null, which
     denies the write. */
  async function loadCapabilities(): Promise<void> {
    try {
      capabilities = (await dlssCapabilities()) ?? null;
    } catch {
      capabilities = null;
    } finally {
      capabilityLoaded = true;
    }
  }

  onMount(() => {
    void refresh();
    void loadCapabilities();
  });

  async function learnMore(url: string): Promise<void> {
    try {
      await openUrl(url);
    } catch (err) {
      showToast("warning", translate(get(locale), "component.dlss.toast.openLinkFailed", { error: String(err) }));
    }
  }

  async function apply(): Promise<void> {
    if (!writable) return;
    busy = true;
    try {
      const outcome = await applyDlssOverride(
        scope,
        writeConfig as Parameters<typeof applyDlssOverride>[1],
        changedIds,
      );
      if (outcome.needs_elevation) {
        showToast("warning", translate(get(locale), "component.dlss.toast.needsElevation"));
      } else if (outcome.preset_evidence?.readback === "matched" && outcome.preset_evidence.write === "accepted") {
        showToast("success", translate(get(locale), "component.dlss.profileSaved"));
      } else {
        showToast("warning", translate(get(locale), "component.dlss.profileUnverified"));
      }
      await refresh();
    } catch (err) {
      showToast("danger", translate(get(locale), "component.dlss.toast.applyFailed", { error: String(err) }));
    } finally {
      busy = false;
    }
  }

  async function reset(): Promise<void> {
    if (!writable) return;
    busy = true;
    try {
      await resetDlssOverride(scope);
      writeConfig = emptyDlssPresetWriteDraft();
      showToast("success", translate(get(locale), "component.dlss.toast.reset"));
      await refresh();
    } catch (err) {
      showToast("danger", translate(get(locale), "component.dlss.toast.resetFailed", { error: String(err) }));
    } finally {
      busy = false;
    }
  }
</script>

<div class="dlss">
  <div class="dlss-head">
    <h4>{scope.scope === "global" ? $t("component.dlss.headingGlobal") : $t("component.dlss.heading")}</h4>
    {#if activeCount > 0}<span class="dlss-active">{$t("component.dlss.activeCount", { count: activeCount })}</span>{/if}
    {#if sourceLabel}<span class="dlss-source">{sourceLabel}</span>{/if}
    <button class="dlss-refresh" onclick={refresh} title={$t("component.dlss.refreshTitle")}>
      {$t("component.dlss.refresh")}
    </button>
  </div>

  {#if capabilityLoaded && !writable}
    <div class="dlss-blocked" data-testid="dlss-write-blocked" role="status">
      <p class="dlss-blocked-head">
        {$t("component.dlss.writeUnavailable")}
      </p>
      {#if gate.eligible === false && gate.capabilityObserved && gate.reasons.length > 0}
        <ul class="dlss-blocked-list">
          {#each gate.reasons as reason (reason)}
            <li data-testid="dlss-block-reason" data-reason={reason}>
              {$t("component.dlss.blocked." + reason)}
            </li>
          {/each}
        </ul>
      {:else}
        <p class="dlss-blocked-list">
          {$t("component.dlss.capabilityUnknown")}
        </p>
      {/if}
    </div>
  {/if}

  {#if readError}<p class="dlss-read-error" role="alert">{readError}</p>{/if}
  {#if capabilities?.profile_access}
    <p class="profile-context">{$t("component.dlss.profileAccess", { version: `${Math.floor(capabilities.profile_access.driver_version / 100)}.${String(capabilities.profile_access.driver_version % 100).padStart(2, "0")}` })}</p>
  {/if}

  <section class="dlss-group">
    <span class="dlss-group-title">{$t("component.dlss.superResolution")}</span>
    <Checkbox
      bind:checked={writeConfig.enable_sr_dll_override}
      label={$t("component.dlss.forceLatestSrDll")}
      disabled={controlsDisabled || !settingAvailable(fieldIds.enable_sr_dll_override)}
    />
    <div class="dlss-field">
      <span class="dlss-field-label">{$t("component.dlss.modelPreset")}</span>
      <div class="dlss-control">
        <Select
          bind:value={writeConfig.sr_preset}
          options={srSelectOptions}
          placeholder={$t("component.dlss.noPresetOverride")}
          ariaLabel={$t("component.dlss.modelPresetAria")}
          disabled={controlsDisabled}
        />
      </div>
    </div>
    {#if srDescription}
      <p class="dlss-help" data-testid="sr-preset-desc">
        {srDescription}
        {#if srHelp}
        <button class="dlss-learn" onclick={() => learnMore(srHelp.sourceUrl)}>{$t("component.dlss.learnMore")}</button>
        {/if}
      </p>
    {/if}
  </section>

  <section class="dlss-group">
    <span class="dlss-group-title">{$t("component.dlss.rayReconstruction")}</span>
    <Checkbox
      bind:checked={writeConfig.enable_rr_dll_override}
      label={$t("component.dlss.forceLatestRrDll")}
      disabled={controlsDisabled || !settingAvailable(fieldIds.enable_rr_dll_override)}
    />
    <div class="dlss-field">
      <span class="dlss-field-label">{$t("component.dlss.modelPreset")}</span>
      <div class="dlss-control">
        <Select
          bind:value={writeConfig.rr_preset}
          options={rrSelectOptions}
          placeholder={$t("component.dlss.noPresetOverride")}
          ariaLabel={$t("component.dlss.rrPresetAria")}
          disabled={controlsDisabled}
        />
      </div>
    </div>
    {#if rrDescription}
      <p class="dlss-help" data-testid="rr-preset-desc">
        {rrDescription}
        {#if rrHelp}
        <button class="dlss-learn" onclick={() => learnMore(rrHelp.sourceUrl)}>{$t("component.dlss.learnMore")}</button>
        {/if}
      </p>
    {/if}
    <p class="dlss-help">{$t("component.dlss.rrNote")}</p>
  </section>

  <section class="dlss-group">
    <span class="dlss-group-title">{$t("component.dlss.frameGeneration")}</span>
    <Checkbox
      bind:checked={writeConfig.enable_fg_dll_override}
      label={$t("component.dlss.forceLatestFgDll")}
      disabled={controlsDisabled || !settingAvailable(fieldIds.enable_fg_dll_override)}
    />
    <div class="dlss-field">
      <span class="dlss-field-label">{$t("component.dlss.modelPreset")}</span>
      <div class="dlss-control">
        <Select
          bind:value={writeConfig.fg_preset}
          options={fgSelectOptions}
          placeholder={$t("component.dlss.noPresetOverride")}
          ariaLabel={$t("component.dlss.modelPresetAria")}
          disabled={controlsDisabled}
        />
      </div>
    </div>
    {#if fgDescription}
      <p class="dlss-help" data-testid="fg-preset-desc">{fgDescription}</p>
    {/if}
    <div class="dlss-field">
      <span class="dlss-field-label">{$t("component.dlss.mode")}</span>
      <div class="dlss-control">
        <Select
          bind:value={writeConfig.fg_mode}
          options={fgModeSelectOptions}
          placeholder={$t("component.dlss.noModeOverride")}
          ariaLabel={$t("component.dlss.modeAria")}
          disabled={controlsDisabled}
        />
      </div>
    </div>
    {#if fgModeHelp}
      <p class="dlss-help">
        {$t("dlss.fgMode." + fgModeHelp.value + ".desc")}
        <button class="dlss-learn" onclick={() => learnMore(fgModeHelp.sourceUrl)}>{$t("component.dlss.learnMore")}</button>
      </p>
    {/if}

    {#if writeConfig.fg_mode === "fixed"}
      <div class="dlss-field">
        <span class="dlss-field-label">{$t("component.dlss.fixedMultiplier")}</span>
        <div class="dlss-control">
          <Select
            bind:value={writeConfig.fg_fixed_count}
            options={fgCountSelectOptions}
            placeholder={$t("component.dlss.appControlled")}
            ariaLabel={$t("component.dlss.fixedMultiplierAria")}
            disabled={controlsDisabled}
          />
        </div>
      </div>
      {#if fgCountHelp}
        <p class="dlss-help">
          {$t("dlss.fgCount." + fgCountHelp.value + ".desc")}
          <button class="dlss-learn" onclick={() => learnMore(fgCountHelp.sourceUrl)}>{$t("component.dlss.learnMore")}</button>
        </p>
      {/if}
    {/if}

    {#if writeConfig.fg_mode === "dynamic"}
      <div class="dlss-field">
        <span class="dlss-field-label">{$t("component.dlss.targetFrameRate")}</span>
        <input
          class="dlss-input"
          type="number"
          min="30"
          max="1000"
          bind:value={writeConfig.fg_dynamic_target_fps}
          placeholder={$t("component.dlss.targetFrameRatePlaceholder")}
          disabled={controlsDisabled}
        />
      </div>
      <p class="dlss-help">
        {$t("component.dlss.dynamicHelp")}
      </p>
    {/if}
  </section>

  <div class="dlss-actions">
    <button class="dlss-apply" data-testid="dlss-apply" onclick={apply} disabled={controlsDisabled || changedIds.length === 0}>{busy ? $t("component.dlss.working") : $t("common.apply")}</button>
    <button class="dlss-reset" data-testid="dlss-reset" onclick={reset} disabled={controlsDisabled}>{$t("component.dlss.resetToDefault")}</button>
  </div>
  <p class="dlss-note">
    {$t("component.dlss.note")}
  </p>

  <!-- Phase 5: the four features are presented separately. Each list is the backend registry for
       that function only: its own identifiers, its own raw values and its own descriptions. Frame
       Generation keeps `Default` (0x00FFFFFE) and `Latest` (0x00FFFFFF) as distinct rows. -->
  {#if presets}
    <section class="dlss-reference" data-testid="dlss-preset-reference">
      <button
        class="dlss-reference-toggle"
        onclick={() => (referenceOpen = !referenceOpen)}
        aria-expanded={referenceOpen}
      >
        {referenceOpen ? $t("view.drivers.showLess") : $t("view.drivers.learnMore")}
      </button>
      {#if referenceOpen}
        {#each PRESET_FEATURES as feature (feature)}
          {@const entries = presetEntries(presets, feature)}
          {@const ids = presetSettingIds(presets, feature)}
          {#if entries.length > 0}
            <div class="dlss-reference-group" data-testid={`preset-registry-${feature}`}>
              <span class="dlss-group-title">
                {$t(FEATURE_TITLE_KEY[feature])}
                {#if ids}
                  <span class="mono dlss-reference-ids">{rawValueHex(ids.overrideId)}</span>
                {/if}
              </span>
              <ul class="dlss-reference-list">
                {#each entries as entry (entry.id)}
                  <li data-testid={`preset-${feature}-${entry.id}`} data-raw={rawValueHex(entry.rawValue)}>
                    <span class="mono dlss-reference-id">{entry.id}</span>
                    <span class="mono dlss-reference-raw">{rawValueHex(entry.rawValue)}</span>
                    <span class="dlss-reference-desc">
                      {entry.description}
                      {#if entry.writeMapping?.status === "read_only"}
                        <span class="dlss-reference-read-only" data-testid={`preset-${feature}-${entry.id}-read-only`}>
                          {$t(presetReadOnlyReasonKey(entry.writeMapping.reason))}
                        </span>
                      {/if}
                    </span>
                  </li>
                {/each}
              </ul>
            </div>
          {/if}
        {/each}
      {/if}
    </section>
  {/if}
</div>

<style>
  .dlss {
    display: flex;
    flex-direction: column;
    gap: 14px;
    container-type: inline-size;
  }
  .dlss-head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .dlss-head h4 {
    font-size: 14px;
    font-weight: 700;
    color: var(--text-primary);
  }
  .dlss-active {
    font-size: 11px;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: var(--radius-full);
    background: var(--update-dim, var(--accent-dim));
    color: var(--update, var(--accent));
  }
  .dlss-source {
    font-size: 11px;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: var(--radius-full);
    background: var(--bg-elevated);
    color: var(--text-secondary);
  }
  .dlss-refresh {
    margin-left: auto;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
    padding: 4px 8px;
    border-radius: var(--radius-sm);
    background: transparent;
    white-space: nowrap;
    transition: color var(--dur-fast) var(--ease), background var(--dur-fast) var(--ease);
  }
  .dlss-refresh:hover { color: var(--accent); background: var(--accent-dim); }
  .dlss-refresh:focus-visible { outline: none; box-shadow: var(--shadow-ring); }
  .dlss-blocked {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12px;
    color: var(--text-secondary);
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    padding: 10px 12px;
  }
  .dlss-blocked-head {
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }
  .dlss-blocked-list {
    margin: 0;
    padding-left: 16px;
    line-height: 1.5;
    color: var(--text-muted);
  }
  .dlss-reference {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .dlss-reference-toggle {
    align-self: flex-start;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
    background: none;
    padding: 0;
  }
  .dlss-reference-toggle:hover { text-decoration: underline; }
  .dlss-reference-toggle:focus-visible { outline: none; box-shadow: var(--shadow-ring); }
  .dlss-reference-group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .dlss-reference-ids {
    margin-left: 8px;
    font-size: 10px;
    color: var(--text-muted);
    text-transform: none;
    letter-spacing: normal;
  }
  .dlss-reference-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .dlss-reference-list li {
    display: flex;
    gap: 8px;
    align-items: baseline;
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-muted);
  }
  .dlss-reference-id {
    min-width: 64px;
    text-transform: uppercase;
    color: var(--text-secondary);
  }
  .dlss-reference-raw { min-width: 86px; }
  .dlss-reference-desc { flex: 1 1 auto; min-width: 0; }
  .dlss-reference-read-only {
    display: block;
    color: var(--warning, #d6a032);
  }
  .dlss-group {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--bg-card);
  }
  .dlss-group-title {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: var(--letter-wider);
    color: var(--text-muted);
  }
  .dlss-control {
    flex: 0 0 auto;
    width: 240px;
    max-width: 60%;
  }
  .dlss-field {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
  }
  .dlss-field-label {
    font-size: 13px;
    color: var(--text-secondary);
    flex: 1 1 auto;
    min-width: 0;
  }
  .dlss-input {
    flex: 0 0 auto;
    width: 240px;
    max-width: 60%;
    height: 34px;
    padding: 0 10px;
    border-radius: var(--radius-md, 8px);
    background: var(--bg-elevated);
    color: var(--text-primary);
    border: 1px solid var(--border);
    font-size: 13px;
  }
  .dlss-help {
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-muted);
    margin: 0;
  }
  .dlss-learn {
    display: inline;
    padding: 0;
    margin-left: 4px;
    background: none;
    border: none;
    color: var(--accent);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
  }
  .dlss-learn:hover {
    text-decoration: underline;
  }
  .dlss-actions {
    display: flex;
    gap: 10px;
  }
  .dlss-apply,
  .dlss-reset {
    height: 36px;
    padding: 0 16px;
    border-radius: var(--radius-lg);
    font-size: 13px;
    font-weight: 600;
  }
  .dlss-apply {
    background: var(--accent);
    color: var(--accent-fg);
  }
  .dlss-apply:hover:not(:disabled) {
    background: var(--accent-hover);
  }
  .dlss-reset {
    background: var(--bg-elevated);
    color: var(--text-secondary);
  }
  .dlss-reset:hover:not(:disabled) {
    color: var(--text-primary);
  }
  .dlss-apply:disabled,
  .dlss-reset:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .dlss-note {
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-muted);
    margin: 0;
  }
  @container (max-width: 380px) {
    .dlss-field {
      flex-direction: column;
      align-items: stretch;
    }
    .dlss-control,
    .dlss-input {
      width: 100%;
      max-width: none;
    }
  }
  .profile-context { color: var(--text-secondary); font-size: 13px; line-height: 1.6; padding: 0 0 18px; }
  .dlss-read-error { color: var(--danger); font-size: 13px; line-height: 1.5; }
  .dlss-group { border: 0; border-top: 1px solid var(--border); border-radius: 0; background: none; padding: 24px 0; }
  .dlss-group-title { text-transform: none; letter-spacing: normal; font-size: 17px; font-weight: 600; color: var(--text-primary); margin-bottom: 16px; }
  .dlss-field { display: grid; grid-template-columns: minmax(140px, 1fr) minmax(180px, 280px); align-items: center; gap: 16px; }
  .dlss-control { width: 100%; min-width: 0; }
  .dlss-help { color: var(--text-secondary); font-size: 13px; line-height: 1.6; max-width: 75ch; }
  .dlss-actions { padding-top: 16px; gap: 12px; }
  .dlss-apply, .dlss-reset { min-height: 42px; padding: 0 20px; border-radius: 10px; box-shadow: none; }
  @container workspace (max-width: 520px) { .dlss-field { grid-template-columns: 1fr; gap: 8px; } }
  @container drawer (max-width: 520px) { .dlss-field { grid-template-columns: 1fr; gap: 8px; } }
</style>
