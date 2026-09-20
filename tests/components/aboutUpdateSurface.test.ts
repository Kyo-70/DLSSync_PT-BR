import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { tick } from "svelte";
import { cleanup, fireEvent, render } from "@testing-library/svelte";
// The updater plugin is mocked centrally in `tests/setup.ts` for both the bare specifier and the
// file it resolves to, so this suite drives the same module the view loads without naming any path
// inside `node_modules`.
import { updaterMock } from "../setup";

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
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
    getSystemInfo: vi.fn(async () => null),
    buildIssueReport: vi.fn(async () => ({ url: "https://example.invalid" })),
  };
});

vi.mock("@/lib/community", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/community")>();
  return { ...actual, fetchStarCount: vi.fn(async () => null), shareDlssync: vi.fn(async () => "copied") };
});

const About = (await import("@/views/About.svelte")).default;

beforeEach(() => {
  updaterMock.reset();
});

afterEach(() => {
  cleanup();
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

function banner(container: HTMLElement): HTMLElement | null {
  return container.querySelector('[data-testid="about-update-status"]');
}

describe("About — app update surface", () => {
  it("shows no update verdict and runs no check before the user asks", async () => {
    const { container } = render(About);
    await settle();
    expect(updaterMock.check).not.toHaveBeenCalled();
    expect(banner(container)).toBeNull();
  });

  it("separates an answered check with no newer release from a failed check", async () => {
    const { container, getByText } = render(About);
    await settle();
    await fireEvent.click(getByText("Check for updates"));
    await settle();
    const upToDate = banner(container)!;
    expect(upToDate.getAttribute("data-status")).toBe("upToDate");
    const upToDateText = upToDate.textContent ?? "";

    updaterMock.setFailure(new Error("endpoint unreachable"));
    await fireEvent.click(getByText("Check for updates"));
    await settle();
    const failed = banner(container)!;
    expect(failed.getAttribute("data-status")).toBe("error");
    expect(failed.textContent).toContain("endpoint unreachable");
    expect(failed.textContent).not.toBe(upToDateText);
  });

  it("keeps an answer that states nothing out of the up-to-date wording", async () => {
    updaterMock.setResult({});
    const { container, getByText } = render(About);
    await settle();
    await fireEvent.click(getByText("Check for updates"));
    await settle();
    const el = banner(container)!;
    expect(el.getAttribute("data-status")).toBe("indeterminate");
    expect(el.className).toContain("is-warning");
  });

  it("reports an available release with its version", async () => {
    updaterMock.setResult({ available: true, version: "1.8.0" });
    const { container, getByText } = render(About);
    await settle();
    await fireEvent.click(getByText("Check for updates"));
    await settle();
    const el = banner(container)!;
    expect(el.getAttribute("data-status")).toBe("available");
    expect(el.textContent).toContain("1.8.0");
  });

  it("does not move focus when the view mounts", async () => {
    const { container } = render(About);
    await settle();
    const doc = container.ownerDocument;
    expect(doc.activeElement === null || doc.activeElement === doc.body).toBe(true);
  });
});
