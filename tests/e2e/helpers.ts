import { expect, type Page } from "./fixtures";

export type ViewName =
  | "library"
  | "catalog"
  | "drivers"
  | "journal"
  | "backups"
  | "settings"
  | "about";

export async function gotoView(page: Page, view: ViewName): Promise<void> {
  // Activity is a secondary action in the Backups header menu.
  if (view === "journal") {
    await page.getByTestId("nav-backups").click();
    await expect(page.getByTestId("view-backups")).toBeVisible();
    await page.locator(".backup-actions > summary").click();
    await page.getByTestId("nav-journal").click();
    await expect(page.getByTestId("view-journal")).toBeVisible();
    return;
  }
  await page.getByTestId(`nav-${view}`).click();
  await expect(page.getByTestId(`view-${view}`)).toBeVisible();
}

export async function openGameCard(page: Page, index: number): Promise<void> {
  const card = page.locator(".game-card").nth(index);
  await expect(card).toBeVisible();
  await card.getByRole("heading").getByRole("button").click();
  await expect(page.locator(".detail-view")).toBeVisible();
}
