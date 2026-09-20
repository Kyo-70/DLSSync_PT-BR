import { E2E_PROTECTED_GAME_NAME, test, expect } from "./fixtures";
import { gotoView, openGameCard } from "./helpers";

/** Directory `fixtures.ts` seeds inside the per-worker hermetic data root. Every path this spec can
 *  reach lives under it, so no installed game is ever a target. */
const FIXTURE_GAMES_DIR = "FixtureGames";

test.describe("game detail", () => {
  test("opens full detail with feature rows and returns via back", async ({ app }, testInfo) => {
    const { page } = app;
    await gotoView(page, "library");

    if ((await page.locator(".game-card").count()) === 0) {
      testInfo.annotations.push({ type: "gated", description: "no games in library" });
      test.skip(true, "no games in library at test time");
      return;
    }

    await openGameCard(page, 0);
    const detail = page.locator(".detail-view");
    await expect(page.locator(".detail-back")).toBeVisible();
    await expect(page.locator(".drawer-body")).toBeVisible();
    await expect(page.locator(".drawer-foot")).toHaveCount(1);

    const featureRows = await page.locator(".feature-row").count();
    if (featureRows > 0) {
      await expect(page.locator(".feature-row").first()).toBeVisible();
      await expect(page.locator(".summary-row")).toHaveCount(1);
    } else {
      testInfo.annotations.push({
        type: "gated",
        description: "first game has no scanned upscaler DLLs; feature rows / summary row absent",
      });
    }

    await page.locator(".detail-back").click();
    await expect(detail).toHaveCount(0);
    await expect(page.locator(".game-card").first()).toBeVisible();
  });

  test("anti-cheat warning is present on a guarded game", async ({ app }, testInfo) => {
    const { page } = app;
    await gotoView(page, "library");

    const protectedCard = page.locator(".game-card", { hasText: E2E_PROTECTED_GAME_NAME }).first();
    await expect(protectedCard).toBeVisible();
    await expect(protectedCard).toContainText("DLSS", { timeout: 30_000 });
    const detail = page.locator(".detail-view");
    await protectedCard.locator(".body").click();
    await expect(detail).toBeVisible();
    await expect(page.locator(".drawer-body .warning-banner, .detail-view .warning-banner").first()).toBeVisible();
    // The apply-risk note rides on the apply selection (`acActive && selectedCount > 0`), and the
    // selection now comes from the component state the runtime publishes. The protection warning
    // is unconditional; the risk note is asserted only when there is something to apply.
    if (await page.locator(".foot-apply").isEnabled()) {
      await expect(page.locator(".ac-apply-risk")).toBeVisible();
    } else {
      testInfo.annotations.push({
        type: "gated",
        description: "no applicable update published for the guarded fixture game; apply-risk note not applicable",
      });
    }
    await page.locator(".detail-back").click();
  });

  test("updates in one click after the anti-cheat confirmation, with no review step and any failure visible", async ({
    app,
  }, testInfo) => {
    // An apply prepares, downloads and verifies, so this case needs more than the default budget.
    test.setTimeout(120_000);
    const { page } = app;
    await gotoView(page, "library");

    const protectedCard = page.locator(".game-card", { hasText: E2E_PROTECTED_GAME_NAME }).first();
    await expect(protectedCard).toContainText("DLSS", { timeout: 30_000 });
    await protectedCard.locator(".body").click();
    await expect(page.locator(".detail-view")).toBeVisible();

    // The only files in reach belong to the seeded fixture game inside the hermetic data root.
    await expect(page.locator(".drawer-path")).toContainText(FIXTURE_GAMES_DIR);

    // The mandatory plan-review modal was removed: updating is one click. Nothing in this flow may
    // render it, in any wording.
    const planReview = page.locator(".plan-modal");
    const apply = page.locator(".foot-apply");
    await expect(planReview).toHaveCount(0);

    if (!(await apply.isEnabled())) {
      testInfo.annotations.push({
        type: "gated",
        description: "runtime published no applicable update for the fixture game; nothing to apply",
      });
      await page.locator(".detail-back").click();
      return;
    }

    await apply.click();
    // Anti-cheat danger is a real safety block and stays visible. It states a ban risk and asks for
    // one confirmation; it is not a plan review and it carries no file list to approve.
    const confirm = page.locator(".ac-apply-confirm");
    await expect(confirm).toBeVisible();
    await expect(confirm.locator(".ac-confirm-text")).not.toBeEmpty();
    await confirm.locator(".ac-confirm-proceed").click();

    // Preparation happens internally, with no dialog between the confirmation and the work.
    await expect(planReview).toHaveCount(0);

    const progress = page.locator('[aria-labelledby="apply-modal-title"]');
    const dangerToast = page.locator(".toast.toast-danger");
    // The run either starts and reports its outcome, or fails during preparation and says so. A
    // silent no-op is a defect, so one of the two surfaces must appear.
    await expect(progress.or(dangerToast).first()).toBeVisible({ timeout: 30_000 });

    if (await progress.isVisible()) {
      // Wait for a terminal verdict instead of a snapshot mid-flight.
      await expect(progress.locator(".head-eyebrow")).toHaveText(
        /Updates finished|Some updates need attention/,
        { timeout: 90_000 },
      );
      const failed = progress.locator(".stat-chip.stat-failed");
      if ((await failed.count()) > 0) {
        // A real failure names itself: the class, the affected files and the raw message stay on
        // screen. None of it may be swallowed into a generic "done". The detail pane shows the
        // errors of the selected group, so open the failed one first.
        await expect(failed.first()).toBeVisible();
        await progress.locator(".group-tile.failed").first().click();
        await expect(progress.locator(".error-block-kind").first()).not.toBeEmpty();
        await expect(progress.locator(".error-block-msg").first()).not.toBeEmpty();
        testInfo.annotations.push({ type: "observed", description: "apply reported a failure, with its reason visible" });
      }
      // Leave the shared worker-scoped app clean for the next spec.
      await progress.locator(".dialog-close").click();
      await expect(progress).toBeHidden();
    } else {
      // Preparation failed before anything was queued. The reason is on screen, not swallowed.
      await expect(dangerToast.locator(".toast-msg").first()).not.toBeEmpty();
      await expect(planReview).toHaveCount(0);
    }

    await expect(planReview).toHaveCount(0);
    await page.locator(".detail-back").click();
    await expect(page.locator(".detail-view")).toHaveCount(0);
  });
});
