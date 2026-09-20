import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import { tick } from "svelte";
import { cleanup, fireEvent, render } from "@testing-library/svelte";
import { DEFAULT_BACKGROUND_CONFIG, type AppSettings } from "@/lib/api";
/** The fake startup entry lives in the central plugin mock in `tests/setup.ts`, registered for both
 *  the bare specifier and the file it resolves to. Nothing here reads or writes a real Windows
 *  startup entry: the mock only changes that in-memory value. */
import { autostartMock } from "../setup";

const saveSettingsSpy = vi.fn(async () => undefined);

let storedRunAtStartup = false;

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
      settings_active_tab: "general",
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
    background: { ...DEFAULT_BACKGROUND_CONFIG, run_at_startup: storedRunAtStartup },
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
  autostartMock.reset();
  storedRunAtStartup = false;
  saveSettingsSpy.mockClear();
  toasts.set([]);
});

afterEach(() => {
  cleanup();
  settings.set(null);
});

async function settle(): Promise<void> {
  // The view loads the autostart plugin with a dynamic import, so a macrotask turn is needed
  // between the microtask flushes before the read-back lands.
  for (let i = 0; i < 8; i += 1) {
    await tick();
    await Promise.resolve();
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
  await tick();
}

function startupRow(container: HTMLElement): { state: string; checkbox: HTMLInputElement; text: string } {
  const text = container.querySelector('[data-testid="autostart-state"]') as HTMLElement;
  const row = text.closest(".row") as HTMLElement;
  return {
    state: text.getAttribute("data-state") ?? "",
    checkbox: row.querySelector('input[type="checkbox"]') as HTMLInputElement,
    text: text.textContent ?? "",
  };
}

function savedStartupValues(): boolean[] {
  return saveSettingsSpy.mock.calls.map((call) => (call[0] as AppSettings).background.run_at_startup);
}

describe("Settings — Windows startup entry", () => {
  it("shows the value read back from the system, not the stored preference", async () => {
    autostartMock.setEntry(true);
    storedRunAtStartup = false;
    const { container } = render(Settings, { props: { onToggleTheme: vi.fn(), currentTheme: "dark" } });
    await settle();

    const row = startupRow(container);
    expect(autostartMock.isEnabled).toHaveBeenCalled();
    expect(row.state).toBe("observed");
    expect(row.checkbox.checked).toBe(true);
  });

  it("marks the row as not verified when the system returns no usable answer", async () => {
    autostartMock.setEntry(undefined);
    const { container } = render(Settings, { props: { onToggleTheme: vi.fn(), currentTheme: "dark" } });
    await settle();

    const row = startupRow(container);
    expect(row.state).toBe("unknown");
    expect(row.text.trim()).not.toBe("");
    expect(row.checkbox.checked).toBe(false);
  });

  it("stores only the confirmed value after a successful change", async () => {
    autostartMock.setEntry(false);
    const { container } = render(Settings, { props: { onToggleTheme: vi.fn(), currentTheme: "dark" } });
    await settle();

    await fireEvent.change(startupRow(container).checkbox, { target: { checked: true } });
    await settle();

    const row = startupRow(container);
    expect(autostartMock.enable).toHaveBeenCalledTimes(1);
    expect(row.state).toBe("observed");
    expect(row.checkbox.checked).toBe(true);
    expect(savedStartupValues()).toEqual([true]);
  });

  it("reports a discrepancy instead of writing a compensating value", async () => {
    autostartMock.setEntry(false);
    autostartMock.setIgnoreWrites(true);
    const { container } = render(Settings, { props: { onToggleTheme: vi.fn(), currentTheme: "dark" } });
    await settle();

    await fireEvent.change(startupRow(container).checkbox, { target: { checked: true } });
    await settle();

    const row = startupRow(container);
    expect(row.state).toBe("mismatch");
    expect(row.checkbox.checked).toBe(false);
    expect(row.text.trim()).not.toBe("");
    expect(savedStartupValues()).not.toContain(true);
    expect(get(toasts).some((toast) => toast.kind === "warning")).toBe(true);
  });

  it("leaves the stored preference untouched when the call fails", async () => {
    autostartMock.setEntry(false);
    autostartMock.setWriteFailure(new Error("registry write denied"));
    const { container } = render(Settings, { props: { onToggleTheme: vi.fn(), currentTheme: "dark" } });
    await settle();

    await fireEvent.change(startupRow(container).checkbox, { target: { checked: true } });
    await settle();

    const row = startupRow(container);
    expect(row.state).toBe("error");
    expect(row.text).toContain("registry write denied");
    expect(saveSettingsSpy).not.toHaveBeenCalled();
    expect(get(toasts).some((toast) => toast.kind === "danger")).toBe(true);
  });
});
