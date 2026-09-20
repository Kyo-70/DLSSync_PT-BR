import { test, expect } from "./fixtures";
import { gotoView } from "./helpers";

const FAMILIES = ["DLSS", "FSR", "XeSS", "Reflex"];

test.describe("catalog", () => {
  test("vendor families and version pickers render", async ({ app }) => {
    const { page } = app;
    await gotoView(page, "catalog");

    await expect(page.getByRole("heading", { name: "Trust Center", exact: true })).toHaveCount(0);
    await expect(page.locator(".catalog-foot")).toContainText("Catalog ready");

    await expect(page.locator(".catalog-vendor").first()).toBeVisible();
    await expect(page.locator(".catalog-family:visible").first()).toBeVisible();

    let visibleFamilies = 0;
    for (const fam of FAMILIES) {
      if (await page.locator(".catalog-family:visible").filter({ hasText: new RegExp(fam, "i") }).count()) visibleFamilies++;
    }
    expect(visibleFamilies).toBeGreaterThanOrEqual(2);

    const picker = page.locator(".catalog-family:visible").first();
    if (await picker.count()) {
      await picker.click();
      await expect(
        page.locator(".glass-dialog, [class*=flyout], [class*=popover], [role=menu]").first(),
      ).toBeVisible();
      await page.keyboard.press("Escape");
    }
  });

  test("DirectStorage catalog search opens the version history", async ({ app }) => {
    const { page } = app;
    await gotoView(page, "catalog");
    await page.locator(".runtime-search input").fill("DirectStorage");
    const microsoftCard = page.getByRole("region", { name: "Microsoft", exact: true });
    await expect(microsoftCard).toBeVisible();
    const directStorage = microsoftCard.getByRole("button", { name: /^View versions of DirectStorage/ }).first();
    await expect(directStorage).toBeVisible();
    await directStorage.click();
    await expect(page.getByRole("button", { name: /Download/i }).first()).toBeVisible();
  });
});
