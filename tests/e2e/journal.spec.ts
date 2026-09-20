import { test, expect } from "./fixtures";
import { gotoView } from "./helpers";

test.describe("operation journal", () => {
  test("renders filters and the persisted startup history", async ({ app }) => {
    const { page } = app;
    await gotoView(page, "journal");

    // The journal moved under Backups as the "Activity" tab, and its wording moved with it:
    // `view.journal.title` is "Activity" and `view.journal.export` is "Copy history". The retired
    // strings "Operation Journal" and "Copy redacted JSON" are no longer in any catalog.
    await expect(page.getByRole("heading", { name: "Activity", exact: true })).toBeVisible();
    await expect(page.getByLabel("Operation")).toBeVisible();
    await expect(page.getByLabel("Result")).toBeVisible();
    await expect(page.getByRole("button", { name: "Copy history", exact: true })).toBeVisible();
    // Either real entries or the empty state, never a blank surface. Both carry a visible message.
    await expect(page.locator(".journal-entry, .journal-empty").first()).toBeVisible();
  });

  test("returns to Backups from the Activity tab", async ({ app }) => {
    const { page } = app;
    await gotoView(page, "journal");

    await page.getByTestId("journal-to-backups").click();
    await expect(page.getByTestId("view-backups")).toBeVisible();
  });
});
