import { afterEach, describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { tick } from "svelte";
import { cleanup, render } from "@testing-library/svelte";
import { DEFAULT_BACKGROUND_CONFIG, type AppSettings } from "@/lib/api";

type TabId = "general" | "updates" | "detection" | "art" | "advanced";

let activeTab: TabId = "general";

function fullSettings(): AppSettings {
  return {
    launcher_overrides: {
      steam: ["C:\\Games\\Steam"],
      epic: [],
      gog: [],
      ubisoft: [],
      ea_desktop: [],
      xbox: [],
      battlenet: [],
      custom: ["C:\\Games\\Custom"],
    },
    update_prefs: {
      update_dlss: true,
      update_dlss_fg: true,
      update_dlss_rr: true,
      update_streamline: false,
      update_reflex: true,
      update_xess: true,
      update_fsr: true,
      update_direct_storage: true,
      create_backups: true,
      auto_apply_all_on_rescan: false,
    },
    ui_prefs: {
      theme: "dark",
      sidebar_collapsed: false,
      grid_density: "comfy",
      sort_order: "default",
      launcher_filter: "all",
      status_filter: "all",
      library_view_mode: "grid",
      library_density: "comfy",
      library_sort: "default",
      backups_group_by: "game",
      settings_active_tab: activeTab,
      command_palette_recent: [],
      show_support_nudge: true,
      language: "en",
    },
    steam_api: { api_key: "", steam_id: "" },
    steamgriddb: { api_key: "" },
    window_state: { width: null, height: null, top: null, left: null, maximized: false },
    blacklist: [],
    ignored: [],
    game_preferences: {},
    advanced: {
      dlss_debug_overlay: false,
      verbose_logs: false,
      allow_unsigned_dlls: false,
      prefer_stable_channel: false,
      apply_concurrency: 2,
    },
    network: { retry_attempts: 3, download_cache_ttl_secs: 300, connect_timeout_secs: 10, chunk_timeout_secs: 60 },
    background: { ...DEFAULT_BACKGROUND_CONFIG },
  } as AppSettings;
}

// The autostart plugin is mocked centrally in `tests/setup.ts`; its default startup entry reads as
// absent, which is all this suite needs.
vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    getSettings: vi.fn(async () => fullSettings()),
    saveSettings: vi.fn(async () => undefined),
    getAppPaths: vi.fn(async () => ({
      root: "",
      backups_dir: "",
      cache_dir: "",
      logs_dir: "",
      settings_dir: "",
      backups_db: "",
      catalog_cache: "",
      settings_file: "",
    })),
    getDlssDebugOverlay: vi.fn(async () => false),
    checkDriverUpdates: vi.fn(async () => []),
  };
});

const Settings = (await import("@/views/Settings.svelte")).default;
const { settings } = await import("@/lib/stores");

afterEach(() => {
  cleanup();
  settings.set(null);
  activeTab = "general";
});

async function settle(): Promise<void> {
  // Both views load their Tauri plugins with a dynamic import, so a macrotask turn is needed
  // between the microtask flushes before the read-back lands.
  for (let i = 0; i < 8; i += 1) {
    await tick();
    await Promise.resolve();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  await tick();
}

/** Accessible name of a form control, restricted to the mechanisms this view uses:
 *  `aria-label`, `aria-labelledby`, a wrapping `<label>` with text, and `<label for>`. A
 *  placeholder is deliberately not accepted. */
function accessibleName(el: Element): string {
  const aria = el.getAttribute("aria-label");
  if (aria && aria.trim()) return aria.trim();
  const labelledBy = el.getAttribute("aria-labelledby");
  if (labelledBy) {
    const text = labelledBy
      .split(/\s+/)
      .map((id) => el.ownerDocument.getElementById(id)?.textContent ?? "")
      .join(" ")
      .trim();
    if (text) return text;
  }
  const wrapping = el.closest("label");
  if (wrapping && (wrapping.textContent ?? "").trim()) return (wrapping.textContent ?? "").trim();
  const id = el.getAttribute("id");
  if (id) {
    const explicit = el.ownerDocument.querySelector(`label[for="${id}"]`);
    if (explicit && (explicit.textContent ?? "").trim()) return (explicit.textContent ?? "").trim();
  }
  return "";
}

async function renderTab(tab: TabId): Promise<HTMLElement> {
  activeTab = tab;
  const { container } = render(Settings, { props: { onToggleTheme: vi.fn(), currentTheme: "dark" } });
  await settle();
  return container;
}

describe("Settings — accessible names", () => {
  it("keeps a resolved label and description on the five advanced numeric entries", async () => {
    const container = await renderTab("advanced");
    const numbers = Array.from(container.querySelectorAll('input[type="number"]'));
    expect(numbers).toHaveLength(5);
    for (const input of numbers) {
      expect(accessibleName(input)).not.toBe("");
      const describedBy = input.getAttribute("aria-describedby") ?? "";
      expect(describedBy).not.toBe("");
      const description = describedBy
        .split(/\s+/)
        .map((id) => container.ownerDocument.getElementById(id)?.textContent ?? "")
        .join(" ")
        .trim();
      expect(description).not.toBe("");
    }
  });

  it.each(["general", "updates", "detection", "art", "advanced"] as const)(
    "gives every form control on the %s tab an accessible name",
    async (tab) => {
      const container = await renderTab(tab);
      const controls = Array.from(container.querySelectorAll("input, select, textarea"));
      expect(controls.length).toBeGreaterThan(0);
      const unnamed = controls.filter((el) => accessibleName(el) === "");
      expect(unnamed.map((el) => el.outerHTML.slice(0, 120))).toEqual([]);
    },
  );

  it("does not move focus when the view mounts", async () => {
    const container = await renderTab("general");
    const doc = container.ownerDocument;
    expect(doc.activeElement === null || doc.activeElement === doc.body).toBe(true);
  });

  it("has no surface that focuses an element without the shared initial-focus marker", () => {
    const views = [
      "../../frontend/src/views/Settings.svelte",
      "../../frontend/src/views/About.svelte",
      "../../frontend/src/components/PerformanceToggles.svelte",
    ];
    for (const relative of views) {
      const source = readFileSync(fileURLToPath(new URL(relative, import.meta.url)), "utf8");
      const movesFocus = /\.focus\s*\(/.test(source) || /use:focusTrap/.test(source);
      const marksInitialFocus = /data-initial-focus/.test(source);
      expect(movesFocus && !marksInitialFocus).toBe(false);
    }
  });
});
