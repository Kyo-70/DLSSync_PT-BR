/** Deferred view loaders.
 *
 *  `App.svelte` renders one view at a time. Importing every view statically puts all seven into the
 *  entry chunk, so the startup payload carries code the user may never open. Each loader below
 *  resolves its view component on first use and caches only a successful result, so a failed load
 *  can be retried.
 *
 *  `Library` stays a static import in `App.svelte`: it is the first painted view. */

function deferred<T>(load: () => Promise<T>): () => Promise<T> {
  let pending: Promise<T> | null = null;
  return () => {
    if (pending === null) {
      pending = load().catch((error: unknown) => {
        pending = null;
        throw error;
      });
    }
    return pending;
  };
}

export const loadCatalogView = deferred(async () => (await import("../views/Catalog.svelte")).default);
export const loadBackupsView = deferred(async () => (await import("../views/Backups.svelte")).default);
export const loadJournalView = deferred(async () => (await import("../views/Journal.svelte")).default);
export const loadDriversView = deferred(async () => (await import("../views/Drivers.svelte")).default);
export const loadSettingsView = deferred(
  async () => (await import("../views/Settings.svelte")).default,
);
export const loadAboutView = deferred(async () => (await import("../views/About.svelte")).default);
