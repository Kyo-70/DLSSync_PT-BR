import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import { tick } from "svelte";
import { cleanup, fireEvent, render } from "@testing-library/svelte";
import { DEFAULT_BACKGROUND_CONFIG, type AppSettings } from "@/lib/api";
// The updater and autostart plugins are mocked centrally in `tests/setup.ts`, for both the bare
// specifier and the file it resolves to, so this suite needs no path inside `node_modules`.
import { updaterMock, autostartMock } from "../setup";

const saveSettingsSpy = vi.fn(async () => undefined);

function fullSettings(): AppSettings {
  return {
    launcher_overrides: { steam: [], epic: [], gog: [], ubisoft: [], ea_desktop: [], xbox: [], battlenet: [], custom: [] },
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
      settings_active_tab: "updates",
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

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    getSettings: vi.fn(async () => fullSettings()),
    saveSettings: (s: AppSettings) => saveSettingsSpy(s),
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
const { settings, toasts } = await import("@/lib/stores");

beforeEach(() => {
  updaterMock.reset();
  autostartMock.reset();
  // This suite is about the update row, so the startup entry answers a plain, readable "off".
  autostartMock.setEntry(false);
  saveSettingsSpy.mockClear();
  toasts.set([]);
});

afterEach(() => {
  cleanup();
  settings.set(null);
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

function statusEl(container: HTMLElement): HTMLElement {
  return container.querySelector('[data-testid="app-update-status"]') as HTMLElement;
}

async function renderAndCheck(): Promise<{ container: HTMLElement }> {
  const { container: root, getByText } = render(Settings, { props: { onToggleTheme: vi.fn(), currentTheme: "dark" } });
  await settle();
  await fireEvent.click(getByText("Check for updates"));
  await settle();
  return { container: root };
}

describe("Settings — app update surface", () => {
  it("starts as not checked, which is not the same as up to date", async () => {
    const { container } = render(Settings, { props: { onToggleTheme: vi.fn(), currentTheme: "dark" } });
    await settle();

    const el = statusEl(container);
    expect(el.getAttribute("data-status")).toBe("unchecked");
    expect(updaterMock.check).not.toHaveBeenCalled();
    expect(el.textContent?.trim()).not.toBe("");
  });

  it("reports an available release", async () => {
    updaterMock.setResult({ available: true, version: "1.8.0" });
    const { container } = await renderAndCheck();
    const el = statusEl(container);
    expect(el.getAttribute("data-status")).toBe("available");
    expect(el.textContent?.trim()).not.toBe("");
    // The row key ships in the eight catalogs with a version placeholder, so the version is on the
    // row itself, and the toast keeps carrying it too.
    expect(el.textContent).toContain("1.8.0");
    expect(get(toasts).some((toast) => toast.message.includes("1.8.0"))).toBe(true);
  });

  it("reports an answered check with no newer release", async () => {
    updaterMock.setResult(null);
    const { container } = await renderAndCheck();
    expect(statusEl(container).getAttribute("data-status")).toBe("upToDate");
  });

  it("does not present an answer that states nothing as up to date", async () => {
    updaterMock.setResult({});
    const { container } = await renderAndCheck();
    const el = statusEl(container);
    expect(el.getAttribute("data-status")).toBe("indeterminate");
    expect(el.className).toContain("update-status-caution");
  });

  it("does not present a failed check as up to date", async () => {
    updaterMock.setFailure(new Error("endpoint unreachable"));
    const { container } = await renderAndCheck();
    const el = statusEl(container);
    expect(el.getAttribute("data-status")).toBe("error");
    expect(el.textContent).toContain("endpoint unreachable");
  });
});
