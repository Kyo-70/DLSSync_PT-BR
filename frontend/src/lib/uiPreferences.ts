import type { AppSettings } from "./api";

type Preferences = AppSettings["ui_prefs"];
type Patch = Partial<Preferences>;

/** Keep presentation responsive while serializing durable, UI-only preference writes. */
export function createUiPreferenceWriter(options: {
  current: () => AppSettings | null;
  publish: (settings: AppSettings) => void;
  read: () => Promise<AppSettings>;
  save: (settings: AppSettings) => Promise<void>;
  failed: (error: unknown) => void;
}) {
  let pending: Patch = {};
  let active: Patch = {};
  let known: Preferences | null = null;
  let tail = Promise.resolve();

  function project(settings: AppSettings): AppSettings {
    return { ...settings, ui_prefs: { ...settings.ui_prefs, ...active, ...pending } };
  }

  function update(patch: Patch): Promise<void> {
    const current = options.current();
    if (!current) return Promise.resolve();
    known ??= current.ui_prefs;
    pending = { ...pending, ...patch };
    options.publish(project(current));
    tail = tail.then(async () => {
      if (!Object.keys(pending).length) return;
      active = pending;
      pending = {};
      try {
        // Preserve fresh backend-owned and non-UI settings, including window state.
        const persisted = await options.read();
        known = persisted.ui_prefs;
        const next = { ...persisted, ui_prefs: { ...persisted.ui_prefs, ...active } };
        await options.save(next);
        known = next.ui_prefs;
        // Never publish a late acknowledgement over a newer local selection.
      } catch (error) {
        const latest = options.current();
        if (latest && known) {
          const rollback: Patch = {};
          for (const key of Object.keys(active) as (keyof Preferences)[]) {
            if (!(key in pending) && latest.ui_prefs[key] === active[key]) {
              Object.assign(rollback, { [key]: known[key] });
            }
          }
          options.publish({ ...latest, ui_prefs: { ...latest.ui_prefs, ...rollback } });
        }
        options.failed(error);
      } finally {
        active = {};
      }
    });
    return tail;
  }

  return { update, project };
}
