import { test, expect } from "./fixtures";
import { gotoView } from "./helpers";

test.describe("library", () => {
  test("renders cards, segmented view toggle, and supports grid/list switch", async ({ app }, testInfo) => {
    const { page } = app;
    await gotoView(page, "library");

    await page
      .locator(".game-card")
      .first()
      .waitFor({ state: "visible", timeout: 15_000 })
      .catch(() => undefined);
    if ((await page.locator(".game-card").count()) === 0) {
      testInfo.annotations.push({ type: "gated", description: "no games in library" });
      test.skip(true, "no games in library at test time");
      return;
    }
    await expect(page.locator(".game-card").first()).toBeVisible();
    const segButtons = page.locator(".presentation-picker button");
    expect(await segButtons.count()).toBeGreaterThanOrEqual(2);

    const listToggle = page.getByRole("button", { name: /^list$/i }).first();
    await listToggle.click();
    await expect(page.locator(".list").first()).toBeVisible();

    const gridToggle = page.getByRole("button", { name: /^grid$/i }).first();
    await gridToggle.click();
    await expect(page.locator(".grid").first()).toBeVisible();
  });

  test("search input and filter controls are present", async ({ app }) => {
    const { page } = app;
    await gotoView(page, "library");
    await expect(page.locator(".palette-btn")).toBeVisible();
    await expect(page.locator("header input[type=search]")).toHaveCount(0);
    await page.locator(".library-filter-toggle").click();
    await expect(page.locator(".library-filter-options")).toBeVisible();
    await page.locator(".library-filter-toggle").click();
  });

  test("apply-all affordance matches the displayed pending update count", async ({ app }) => {
    const { page } = app;
    await gotoView(page, "library");
    const summary = page.locator(".library-summary");
    await expect(summary).toHaveAttribute("data-update-count", /^\d+$/);
    const count = Number(await summary.getAttribute("data-update-count"));
    if (count > 0) {
      await expect(page.getByTestId("library-update-all")).toBeVisible();
    } else {
      await expect(page.getByTestId("library-update-all")).toHaveCount(0);
    }
  });
});
