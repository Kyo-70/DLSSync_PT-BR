/** App-update availability, shared by the Settings and About surfaces.
 *
 *  This module is the single owner of the classifier and of the wording of each outcome. It was
 *  duplicated in `views/Settings.svelte` and `views/About.svelte` because those two views are code
 *  split and importing one view into the other would merge their chunks. A shared leaf module
 *  removes the duplication without that cost: it imports no view, so neither view chunk can pull
 *  in the other.
 *
 *  Its only dependency is the message catalog, which the entry chunk already carries. Every key
 *  used here ships in the eight catalogs, so each outcome reads its own translated wording
 *  directly; there is no weaker stand-in left in this file. */
import { translate, type Locale } from "./i18n/index";

/** Availability of an app update. Every outcome stays apart on purpose:
 *  `upToDate` means the updater answered and reported no newer release; `indeterminate` means the
 *  updater answered with a shape that states nothing; `error` means the check did not complete.
 *  Neither of the last two may be presented as "up to date".
 *  `manualOnly` is the distribution policy of a channel without a self-updater; it is not a
 *  measurement and never triggers an automatic check. */
export type AppUpdateAvailability =
  | { kind: "unchecked" }
  | { kind: "checking" }
  | { kind: "available"; version: string | null }
  | { kind: "upToDate" }
  | { kind: "indeterminate"; reason: "unexpected-shape" | "no-availability-field" }
  | { kind: "error"; error: string }
  | { kind: "manualOnly" };

/** Classify the raw updater answer without collapsing distinct outcomes.
 *  `null`/`undefined` is the updater's documented "no newer release" answer. Any other shape that
 *  does not state availability stays `indeterminate`. */
export function classifyUpdateCheck(result: unknown): AppUpdateAvailability {
  if (result === null || result === undefined) return { kind: "upToDate" };
  if (typeof result !== "object") return { kind: "indeterminate", reason: "unexpected-shape" };
  const record = result as { available?: unknown; version?: unknown };
  const version =
    typeof record.version === "string" && record.version.trim() ? record.version.trim() : null;
  if (record.available === false) return { kind: "upToDate" };
  if (record.available === true) return { kind: "available", version };
  if (record.available === undefined) {
    return version
      ? { kind: "available", version }
      : { kind: "indeterminate", reason: "no-availability-field" };
  }
  return { kind: "indeterminate", reason: "unexpected-shape" };
}

/** Text for the Settings update row. Every branch names its own outcome; no branch reuses the
 *  wording of another one. */
export function updateStatusText(
  loc: Locale,
  status: AppUpdateAvailability,
  appVersion: string,
  distribution: string,
): string {
  switch (status.kind) {
    case "unchecked":
      return translate(loc, "view.settings.updates.status.unchecked");
    case "checking":
      return translate(loc, "view.settings.updates.status.checking");
    case "available":
      return translate(loc, "view.settings.updates.status.available", {
        // The updater can report an available release without naming its version. The word is
        // translated, so no English literal reaches the screen.
        version: status.version ?? translate(loc, "status.unknown"),
      });
    case "upToDate":
      return translate(loc, "view.settings.updates.status.upToDate", { version: appVersion });
    case "indeterminate":
      return translate(loc, "view.settings.updates.status.indeterminate");
    case "error":
      // `error` is the raw failure text from the updater, an open wire string: it is inserted as a
      // value, never translated.
      return translate(loc, "view.settings.updates.status.error", { error: status.error });
    case "manualOnly":
      return translate(loc, "view.settings.updates.status.manualOnly", { distribution });
  }
}

export type UpdateBannerTone = "info" | "success" | "warning" | "danger";

/** Banner content for one availability outcome on the About surface. Each outcome keeps its own
 *  tone and wording. `unchecked` draws no banner, and so does `manualOnly`: that surface never
 *  runs a check, so inventing a verdict for it would state something no measurement supports. */
export function updateBannerFor(
  loc: Locale,
  status: AppUpdateAvailability,
  appVersion: string,
): { tone: UpdateBannerTone; text: string } | null {
  switch (status.kind) {
    case "unchecked":
    case "manualOnly":
      return null;
    case "checking":
      return { tone: "info", text: translate(loc, "view.about.update.checking") };
    case "available":
      return {
        tone: "info",
        text: translate(loc, "view.about.update.available", {
          version: status.version ?? translate(loc, "status.unknown"),
        }),
      };
    case "upToDate":
      return {
        tone: "success",
        text: translate(loc, "view.about.update.latest", { version: appVersion }),
      };
    case "indeterminate":
      return {
        tone: "warning",
        text: translate(loc, "view.about.update.status.indeterminate"),
      };
    case "error":
      return { tone: "danger", text: translate(loc, "view.about.update.failed", { error: status.error }) };
  }
}
