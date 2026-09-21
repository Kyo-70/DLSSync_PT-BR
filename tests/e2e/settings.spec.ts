import { test, expect } from "./fixtures";
import { appVersion } from "./config";
import { gotoView, selectSettingsTab } from "./helpers";

test.describe("settings", () => {
  test("file details, section headings, toggles and tab switching work", async ({ app }) => {
    const { page } = app;
    await gotoView(page, "settings");

    await page.locator(".settings-files > summary").click();
    await expect(page.locator(".settings-file-meta")).toContainText(`v${appVersion}`);
    await page.locator(".settings-files > summary").click();

    expect(await page.locator(".section-title-h").count()).toBeGreaterThanOrEqual(2);
    expect(await page.locator(".side-tab input[type=checkbox], .card input[type=checkbox], .seg-btn").count()).toBeGreaterThan(0);

    await selectSettingsTab(page, "updates");
    await expect(page.locator(".section-title-h").first()).toBeVisible();
  });
});
