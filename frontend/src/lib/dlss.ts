import type {
  CapabilityAssessment,
  DlssCapabilitySnapshot,
  DlssOverrideConfig,
  DlssPreset,
  DlssPresetRegistry,
  FgPreset,
  FrameGenCount,
  FrameGenMode,
  RrPreset,
  SrPreset,
} from "./api";

export interface Option<T> {
  value: T;
  label: string;
  description: string;
  sourceUrl: string;
}

const SRC = {
  streamline: "https://github.com/NVIDIAGameWorks/Streamline/blob/main/include/sl_dlss.h",
  dlss45:
    "https://www.nvidia.com/en-us/geforce/news/dlss-4-5-dynamic-multi-frame-gen-6x-2nd-gen-transformer-super-res/",
  dlssOverview: "https://developer.nvidia.com/rtx/dlss",
  presetTable: "https://en.wikipedia.org/wiki/Nvidia_DLSS",
  dynamicMfg:
    "https://www.nvidia.com/en-us/geforce/news/dlss-4-5-dynamic-multi-frame-generation-6x-mode-released/",
} as const;

export const SR_PRESET_OPTIONS: Option<DlssPreset>[] = [
  {
    value: "recommended",
    label: "Recommended",
    description:
      "Super Resolution uses the legacy Recommended token. It is not the provider Latest preset and is not offered by the typed write mapping.",
    sourceUrl: SRC.dlss45,
  },
  {
    value: "default",
    label: "Use app default",
    description: "Leave the game's own DLSS model untouched — no preset is forced.",
    sourceUrl: SRC.dlssOverview,
  },
  {
    value: "k",
    label: "Preset K — Transformer (latest)",
    description:
      "DLSS 4 transformer model. Best image quality — sharper, more stable, less ghosting — at a higher GPU cost. Default for DLAA / Quality / Balanced.",
    sourceUrl: SRC.streamline,
  },
  {
    value: "j",
    label: "Preset J — Transformer",
    description:
      "DLSS 4 transformer, close to K with slightly less ghosting but a touch more flicker. K is generally preferred over J.",
    sourceUrl: SRC.streamline,
  },
  {
    value: "l",
    label: "Preset L — Transformer (Ultra Perf)",
    description:
      "DLSS 4.5 second-gen transformer tuned for Ultra Performance / 4K — sharpest and most stable, highest cost. RTX 20/30 lack FP8 so it is heavier there.",
    sourceUrl: SRC.dlss45,
  },
  {
    value: "m",
    label: "Preset M — Transformer (Perf)",
    description:
      "DLSS 4.5 second-gen transformer tuned for Performance mode — near-L quality at roughly J/K speed.",
    sourceUrl: SRC.dlss45,
  },
  {
    value: "e",
    label: "Preset E — CNN (legacy)",
    description:
      "Legacy convolutional model. Prefer a transformer preset (K) on RTX unless a specific game misbehaves with it.",
    sourceUrl: SRC.presetTable,
  },
  {
    value: "f",
    label: "Preset F — CNN (legacy)",
    description: "Legacy CNN tuned for Ultra Performance / DLAA at 4K and above.",
    sourceUrl: SRC.presetTable,
  },
  {
    value: "c",
    label: "Preset C — CNN (legacy)",
    description:
      "Legacy CNN variant for fast-paced games — less ghosting at the cost of temporal stability.",
    sourceUrl: SRC.presetTable,
  },
  {
    value: "d",
    label: "Preset D — CNN (legacy)",
    description:
      "Legacy CNN variant for slower-paced games — more temporally stable but more ghosting.",
    sourceUrl: SRC.presetTable,
  },
  {
    value: "n",
    label: "Preset N — Transformer",
    description: "DLSS transformer preset N. Shown so a profile set in the NVIDIA app or Profile Inspector is respected.",
    sourceUrl: SRC.dlss45,
  },
  {
    value: "o",
    label: "Preset O — Transformer",
    description: "DLSS transformer preset O. Shown so a profile set in the NVIDIA app or Profile Inspector is respected.",
    sourceUrl: SRC.dlss45,
  },
  {
    value: "a",
    label: "Preset A — CNN (legacy)",
    description: "Legacy CNN preset. Prefer a transformer preset on RTX unless a game misbehaves.",
    sourceUrl: SRC.presetTable,
  },
  {
    value: "b",
    label: "Preset B — CNN (legacy)",
    description: "Legacy CNN preset. Prefer a transformer preset on RTX unless a game misbehaves.",
    sourceUrl: SRC.presetTable,
  },
  {
    value: "g",
    label: "Preset G — CNN (legacy)",
    description: "Legacy CNN preset. Prefer a transformer preset on RTX unless a game misbehaves.",
    sourceUrl: SRC.presetTable,
  },
  {
    value: "h",
    label: "Preset H — CNN (legacy)",
    description: "Legacy CNN preset. Prefer a transformer preset on RTX unless a game misbehaves.",
    sourceUrl: SRC.presetTable,
  },
  {
    value: "i",
    label: "Preset I — CNN (legacy)",
    description: "Legacy CNN preset. Prefer a transformer preset on RTX unless a game misbehaves.",
    sourceUrl: SRC.presetTable,
  },
];

export const FG_MODE_OPTIONS: Option<FrameGenMode>[] = [
  {
    value: "app_controlled",
    label: "Use app setting",
    description: "Keep the in-game Frame Generation setting unchanged.",
    sourceUrl: SRC.dlssOverview,
  },
  {
    value: "fixed",
    label: "Fixed multiplier",
    description:
      "Force a fixed multiplier (2×/3×/4×, up to 6× in supported titles). 'App controlled' count keeps the in-game 2×.",
    sourceUrl: SRC.dynamicMfg,
  },
  {
    value: "dynamic",
    label: "Dynamic (DLSS 4.5)",
    description:
      "Automatically shifts the multiplier to hit your target frame rate / refresh rate. RTX 50 only. Not compatible with frame-rate limiters or V-Sync.",
    sourceUrl: SRC.dynamicMfg,
  },
];

export const FG_COUNT_OPTIONS: Option<FrameGenCount>[] = [
  {
    value: "app_controlled",
    label: "App controlled",
    description: "Let the game/driver choose the Frame Generation multiplier.",
    sourceUrl: SRC.dlssOverview,
  },
  {
    value: "x2",
    label: "2× Frame Generation",
    description: "One generated frame per rendered frame. Supported on RTX 40 and RTX 50.",
    sourceUrl: SRC.dynamicMfg,
  },
  {
    value: "x3",
    label: "3× Multi Frame Generation",
    description: "Two generated frames per rendered frame. RTX 50 only.",
    sourceUrl: SRC.dynamicMfg,
  },
  {
    value: "x4",
    label: "4× Multi Frame Generation",
    description: "Three generated frames per rendered frame. RTX 50 only.",
    sourceUrl: SRC.dynamicMfg,
  },
];

export const DYNAMIC_MFG_MIN_DRIVER_PACKED = 59597;
export const DLSS5_MIN_DRIVER_PACKED = 61047;

export const RR_PRESET_OPTIONS: Option<DlssPreset>[] = [
  {
    value: "recommended",
    label: "Recommended",
    description:
      "Ray Reconstruction uses the legacy Recommended token. It is not the provider Latest preset and is not offered by the typed write mapping.",
    sourceUrl: SRC.dlss45,
  },
  {
    value: "default",
    label: "Use app default",
    description: "Leave the game's own Ray Reconstruction model untouched — no preset is forced.",
    sourceUrl: SRC.dlssOverview,
  },
  {
    value: "k",
    label: "Preset K — Transformer (latest)",
    description:
      "Latest transformer Ray Reconstruction model. Cleanest denoising with the least smearing — at a higher GPU cost.",
    sourceUrl: SRC.streamline,
  },
  {
    value: "j",
    label: "Preset J — Transformer",
    description:
      "Transformer Ray Reconstruction, close to K with slightly different ghosting behavior. K is generally preferred.",
    sourceUrl: SRC.streamline,
  },
  ...SR_PRESET_OPTIONS.filter((o) =>
    ["l", "m", "e", "f", "c", "d", "n", "o", "a", "b", "g", "h", "i"].includes(o.value),
  ),
];

export function emptyDlssConfig(): DlssOverrideConfig {
  return {
    enable_sr_dll_override: false,
    sr_preset: null,
    enable_rr_dll_override: false,
    rr_preset: null,
    enable_fg_dll_override: false,
    fg_preset: null,
    fg_mode: null,
    fg_fixed_count: null,
    fg_dynamic_target_fps: null,
  };
}

export function presetLabel(preset: DlssPreset): string {
  return SR_PRESET_OPTIONS.find((option) => option.value === preset)?.label ?? preset.toUpperCase();
}

export function dynamicMfgAvailable(driverPacked: number): boolean {
  return driverPacked >= DYNAMIC_MFG_MIN_DRIVER_PACKED;
}

export function hasActiveOverride(config: DlssOverrideConfig): boolean {
  return (
    config.enable_sr_dll_override ||
    config.enable_fg_dll_override ||
    config.enable_rr_dll_override ||
    config.sr_preset != null ||
    config.fg_preset != null ||
    config.rr_preset != null ||
    config.fg_mode != null ||
    config.fg_fixed_count != null ||
    config.fg_dynamic_target_fps != null
  );
}


export function dlss5Available(driverPacked: number): boolean {
  return driverPacked >= DLSS5_MIN_DRIVER_PACKED;
}

export interface DlssPresetWriteDraft {
  enable_sr_dll_override: boolean;
  sr_preset: SrPreset | null;
  enable_rr_dll_override: boolean;
  rr_preset: RrPreset | null;
  enable_fg_dll_override: boolean;
  fg_preset: FgPreset | null;
  fg_mode: FrameGenMode | null;
  fg_fixed_count: FrameGenCount | null;
  fg_dynamic_target_fps: number | null;
}

export function emptyDlssPresetWriteDraft(): DlssPresetWriteDraft {
  return {
    enable_sr_dll_override: false,
    sr_preset: null,
    enable_rr_dll_override: false,
    rr_preset: null,
    enable_fg_dll_override: false,
    fg_preset: null,
    fg_mode: null,
    fg_fixed_count: null,
    fg_dynamic_target_fps: null,
  };
}

const LEGACY_LETTER_PRESETS = new Set([
  "a",
  "b",
  "c",
  "d",
  "e",
  "f",
  "g",
  "h",
  "i",
  "j",
  "k",
  "l",
  "m",
  "n",
  "o",
]);

function legacyPresetForTypedReadback(value: DlssPreset | null): string | null {
  if (value === "default") return "off";
  if (value !== null && LEGACY_LETTER_PRESETS.has(value)) return value;
  return null;
}

/** Convert only legacy values whose provider meaning is exact. The legacy `recommended` token is
 * not converted to provider `latest`; incomplete readback stays incomplete instead of guessing. */
export function presetWriteDraftFromReadback(config: DlssOverrideConfig): DlssPresetWriteDraft {
  return {
    enable_sr_dll_override: config.enable_sr_dll_override,
    sr_preset: legacyPresetForTypedReadback(config.sr_preset) as SrPreset | null,
    enable_rr_dll_override: config.enable_rr_dll_override,
    rr_preset: legacyPresetForTypedReadback(config.rr_preset) as RrPreset | null,
    enable_fg_dll_override: config.enable_fg_dll_override,
    fg_preset: legacyPresetForTypedReadback(config.fg_preset) as FgPreset | null,
    fg_mode: config.fg_mode,
    fg_fixed_count: config.fg_fixed_count,
    fg_dynamic_target_fps: config.fg_dynamic_target_fps,
  };
}

/* ------------------------------------------------------------------------------------------- *
 * Phase-5 capability and preset contract (`DlssCapabilitySnapshot`).
 *
 * The four DLSS features keep separate preset ranges, numeric namespaces and descriptions, so
 * nothing below merges them into a shared table. `CapabilityAssessment.write_eligible` is the only
 * gate that authorises a DRS write; `provider_documented_namespace` is a documentation fact, not a
 * support claim, and is never promoted here.
 * ------------------------------------------------------------------------------------------- */

export type PresetFeature = "sr" | "rr" | "fg" | "nr";

export const PRESET_FEATURES: readonly PresetFeature[] = ["sr", "rr", "fg", "nr"];

/** One backend-published preset of one feature: its own identifier, raw value and description. */
export interface PresetEntry {
  feature: PresetFeature;
  id: string;
  rawValue: number;
  description: string;
  writeMapping: PresetWriteMapping | null;
}

export type PresetReadOnlyReason =
  "provider_runtime_write_support_unverified";

export type PresetWriteMapping =
  | { status: "writable" }
  | { status: "read_only"; reason: PresetReadOnlyReason };

/** Raw DRS values the backend publishes for the Frame Generation namespace. `Default` and `Latest`
 *  are different values and are never folded together. */
export const FG_PRESET_DEFAULT_RAW = 0x00fffffe;
export const PRESET_LATEST_RAW = 0x00ffffff;

function entries<T extends { raw_value: number; description: string }>(
  feature: PresetFeature,
  options: readonly (T & { preset: string })[] | undefined,
): PresetEntry[] {
  return (options ?? []).map((option) => ({
    feature,
    id: option.preset,
    rawValue: option.raw_value,
    description: option.description,
    writeMapping:
      ((option as T & { write_mapping?: PresetWriteMapping }).write_mapping as
        | PresetWriteMapping
        | undefined) ?? null,
  }));
}

export interface WritablePresetOption {
  value: string;
  providerPreset: string;
  description: string;
}

/** Typed provider presets that the backend accepts through the feature-specific write DTO. */
export function writablePresetOptions(
  registry: DlssPresetRegistry | null | undefined,
  feature: Exclude<PresetFeature, "nr">,
): WritablePresetOption[] {
  return presetEntries(registry, feature).flatMap((entry) =>
    entry.writeMapping?.status === "writable"
      ? [
          {
            value: entry.id,
            providerPreset: entry.id,
            description: entry.description,
          },
        ]
      : [],
  );
}

export function presetReadOnlyReasonKey(reason: PresetReadOnlyReason): string {
  switch (reason) {
    case "provider_runtime_write_support_unverified":
      return "component.dlss.blocked.preset_runtime_mapping_unknown";
  }
}

/** Project one feature's registry. Each feature is read from its own branch, so a description or a
 *  raw value can never leak from another feature. */
export function presetEntries(
  registry: DlssPresetRegistry | null | undefined,
  feature: PresetFeature,
): PresetEntry[] {
  if (!registry) return [];
  switch (feature) {
    case "sr":
      return entries("sr", registry.sr?.options);
    case "rr":
      return entries("rr", registry.rr?.options);
    case "fg":
      return entries("fg", registry.fg?.options);
    case "nr":
      return entries("nr", registry.nr?.options);
  }
}

export function presetEntry(
  registry: DlssPresetRegistry | null | undefined,
  feature: PresetFeature,
  id: string,
): PresetEntry | null {
  return presetEntries(registry, feature).find((entry) => entry.id === id) ?? null;
}

/** The setting-id pair the backend publishes for a feature namespace, or null when absent. */
export function presetSettingIds(
  registry: DlssPresetRegistry | null | undefined,
  feature: PresetFeature,
): { overrideId: number; presetId: number } | null {
  if (!registry) return null;
  const ids =
    feature === "sr"
      ? registry.sr?.setting_ids
      : feature === "rr"
        ? registry.rr?.setting_ids
        : feature === "fg"
          ? registry.fg?.setting_ids
          : registry.nr?.setting_ids;
  if (!ids) return null;
  return { overrideId: ids.override_id, presetId: ids.preset_id };
}

/** `0x` + eight upper-case hex digits. A machine identifier, shown as an identifier. */
export function rawValueHex(rawValue: number): string {
  return `0x${(rawValue >>> 0).toString(16).toUpperCase().padStart(8, "0")}`;
}

/** Backend description of `preset` **in this feature only**. Returns null when the feature's
 *  registry does not publish that identifier, so a caller can never borrow another feature's text. */
export function featurePresetDescription(
  registry: DlssPresetRegistry | null | undefined,
  feature: PresetFeature,
  preset: DlssPreset | string | null,
): string | null {
  if (preset === null) return null;
  return presetEntry(registry, feature, preset)?.description ?? null;
}

/** Local Ray Reconstruction copy that is written for Ray Reconstruction. Used only until the
 *  backend registry is available; the shared Super Resolution rows are deliberately excluded. */
export const RR_SPECIFIC_PRESETS: readonly string[] = ["recommended", "default", "k", "j"];

export function rrLocalDescription(preset: DlssPreset | RrPreset | null): string | null {
  if (preset === null || !RR_SPECIFIC_PRESETS.includes(preset)) return null;
  return RR_PRESET_OPTIONS.find((option) => option.value === (preset as DlssPreset))?.description ?? null;
}

export type CapabilityBlockReason = CapabilityAssessment["block_reasons"][number];

/** Whether a DRS write may be offered at all.
 *
 *  Fail-closed: an absent snapshot, an absent adapter list and an adapter whose assessment is not
 *  `write_eligible` all deny the write. `capabilityObserved` distinguishes "the backend answered
 *  and said no" from "no capability answer was obtained", so the reason shown stays accurate
 *  without claiming compatibility either way. */
export type DlssWriteGate =
  | { eligible: true; adapterModel: string }
  | { eligible: false; capabilityObserved: boolean; reasons: CapabilityBlockReason[] };

export function dlssWriteGate(
  snapshot: DlssCapabilitySnapshot | null | undefined,
): DlssWriteGate {
  const adapters = snapshot?.adapters ?? [];
  if (adapters.length === 0) return { eligible: false, capabilityObserved: false, reasons: [] };
  // Saving a driver profile is separate from proof that a game uses that setting.
  const profileAdapter = adapters.find(adapter => adapter.evidence.adapter.provider.provider === "nvidia"
    && adapter.evidence.adapter.hardware_recognized);
  if (profileAdapter && snapshot?.profile_access?.setting_ids.length) {
    return { eligible: true, adapterModel: profileAdapter.adapter_model };
  }
  const allowed = adapters.find((adapter) => adapter.assessment?.write_eligible === true);
  if (allowed) return { eligible: true, adapterModel: allowed.adapter_model };
  const reasons: CapabilityBlockReason[] = [];
  for (const adapter of adapters) {
    for (const reason of adapter.assessment?.block_reasons ?? []) {
      if (!reasons.includes(reason)) reasons.push(reason);
    }
  }
  return { eligible: false, capabilityObserved: true, reasons };
}
