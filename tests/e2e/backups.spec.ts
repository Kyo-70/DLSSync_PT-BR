import { test, expect } from "./fixtures";
import { gotoView } from "./helpers";

test.describe("backups", () => {
  test("compact header, actions, search and groups render without a statistics panel", async ({ app }) => {
    const { page } = app;
    await gotoView(page, "backups");
    await expect(page.getByRole("heading", { name: "Backups", exact: true })).toBeVisible();
    await expect(page.locator(".backup-hero, .backup-legend")).toHaveCount(0);
    const actions = page.locator(".backup-actions > summary");
    await actions.click();
    await expect(page.getByTestId("nav-journal")).toBeVisible();
    await actions.click();

    const hasGroups = (await page.locator(".group-row").count()) > 0;
    const hasEmpty = (await page.locator(".empty").count()) > 0;
    expect(hasGroups || hasEmpty).toBe(true);

    if (hasGroups) {
      await expect(page.locator(".backup-search input").first()).toBeVisible();
      await expect(page.locator(".group-by-toggle").first()).toBeVisible();
    }
  });
});
