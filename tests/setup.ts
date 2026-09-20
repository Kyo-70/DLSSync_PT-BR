import { vi, type Mock } from "vitest";

/** Controls for the Tauri plugins the app loads with a dynamic import.
 *
 *  Both plugins are mocked here, once, for every suite. Two module ids are registered for each
 *  plugin: the bare specifier the views import, and the exact file inside `frontend/node_modules`
 *  that specifier resolves to. Registering both is what lets a suite living outside `frontend/`
 *  drive the same module the view loads, so no suite has to name a path inside `node_modules`.
 *
 *  The defaults are the quiet ones: the updater answers "no newer release" and the startup entry
 *  reads as absent. A suite changes them through the exported controls and calls `reset()` in its
 *  own `beforeEach`. */
const plugins = vi.hoisted(() => {
  const updaterState = { result: null as unknown, failure: null as unknown };
  const autostartState = {
    /** Value the fake startup entry reports. `undefined` means it could not be read. */
    entry: undefined as unknown,
    writeFailure: null as unknown,
    ignoreWrites: false,
  };

  const check = vi.fn(async (): Promise<unknown> => {
    if (updaterState.failure) throw updaterState.failure;
    return updaterState.result;
  });

  const enable = vi.fn(async (): Promise<void> => {
    if (autostartState.writeFailure) throw autostartState.writeFailure;
    if (!autostartState.ignoreWrites) autostartState.entry = true;
  });
  const disable = vi.fn(async (): Promise<void> => {
    if (autostartState.writeFailure) throw autostartState.writeFailure;
    if (!autostartState.ignoreWrites) autostartState.entry = false;
  });
  const isEnabled = vi.fn(async (): Promise<unknown> => autostartState.entry);

  const updaterMock = {
    check,
    /** Raw answer every following `check()` resolves with. */
    setResult(result: unknown): void {
      updaterState.result = result;
      updaterState.failure = null;
    },
    /** Error every following `check()` rejects with. */
    setFailure(error: unknown): void {
      updaterState.failure = error;
    },
    reset(): void {
      updaterState.result = null;
      updaterState.failure = null;
      check.mockClear();
    },
  };

  const autostartMock = {
    enable,
    disable,
    isEnabled,
    /** Value the startup entry reports. Pass `undefined` for an entry that cannot be read. */
    setEntry(value: unknown): void {
      autostartState.entry = value;
    },
    /** Make `enable`/`disable` reject, leaving the entry untouched. */
    setWriteFailure(error: unknown): void {
      autostartState.writeFailure = error;
    },
    /** Accept a write and leave the entry unchanged, so a read-back disagrees with the request. */
    setIgnoreWrites(ignore: boolean): void {
      autostartState.ignoreWrites = ignore;
    },
    /** Value the fake entry currently holds. */
    entry(): unknown {
      return autostartState.entry;
    },
    reset(): void {
      autostartState.entry = undefined;
      autostartState.writeFailure = null;
      autostartState.ignoreWrites = false;
      enable.mockClear();
      disable.mockClear();
      isEnabled.mockClear();
    },
  };

  return {
    updaterMock,
    autostartMock,
    updaterModule: { check: () => check() },
    autostartModule: {
      enable: () => enable(),
      disable: () => disable(),
      isEnabled: () => isEnabled(),
    },
  };
});

export interface UpdaterMock {
  check: Mock<() => Promise<unknown>>;
  setResult(result: unknown): void;
  setFailure(error: unknown): void;
  reset(): void;
}

export interface AutostartMock {
  enable: Mock<() => Promise<void>>;
  disable: Mock<() => Promise<void>>;
  isEnabled: Mock<() => Promise<unknown>>;
  setEntry(value: unknown): void;
  setWriteFailure(error: unknown): void;
  setIgnoreWrites(ignore: boolean): void;
  entry(): unknown;
  reset(): void;
}

export const updaterMock: UpdaterMock = plugins.updaterMock;
export const autostartMock: AutostartMock = plugins.autostartMock;

const tauriInvoke = async (cmd: string): Promise<unknown> => {
  switch (cmd) {
    case "list_notifications":
      return [];
    case "notifications_unread_count":
      return 0;
    case "mark_all_notifications_read":
      return 0;
    case "plugin:event|listen":
      return 0;
    default:
      return undefined;
  }
};

if (typeof globalThis !== "undefined") {
  const internals = {
    invoke: vi.fn(tauriInvoke),
    transformCallback: (cb: unknown) => {
      const id = Math.floor(Math.random() * 1e9);
      (globalThis as Record<string, unknown>)[`_${id}`] = cb;
      return id;
    },
    convertFileSrc: (p: string) => p,
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
  };
  const eventInternals = { unregisterListener: () => undefined };
  (globalThis as Record<string, unknown>).__TAURI_INTERNALS__ = internals;
  (globalThis as Record<string, unknown>).__TAURI_EVENT_PLUGIN_INTERNALS__ = eventInternals;
  if (typeof window !== "undefined") {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = internals;
    (window as unknown as Record<string, unknown>).__TAURI_EVENT_PLUGIN_INTERNALS__ = eventInternals;
  }
}

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    switch (cmd) {
      case "list_notifications":
        return [];
      case "notifications_unread_count":
        return 0;
      case "mark_all_notifications_read":
        return 0;
      default:
        return undefined;
    }
  }),
  convertFileSrc: vi.fn((p: string) => p),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
  emit: vi.fn(async () => undefined),
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(async () => "0.0.0-test"),
}));

vi.mock("@tauri-apps/plugin-updater", () => plugins.updaterModule);
vi.mock("../frontend/node_modules/@tauri-apps/plugin-updater/dist-js/index.js", () => plugins.updaterModule);

vi.mock("@tauri-apps/plugin-autostart", () => plugins.autostartModule);
vi.mock("../frontend/node_modules/@tauri-apps/plugin-autostart/dist-js/index.js", () => plugins.autostartModule);

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(async () => null),
  confirm: vi.fn(async () => true),
}));

vi.mock("@tauri-apps/plugin-shell", () => ({
  open: vi.fn(async () => undefined),
}));

if (!("randomUUID" in globalThis.crypto)) {
  let counter = 0;
  Object.defineProperty(globalThis.crypto, "randomUUID", {
    value: () => `00000000-0000-4000-8000-${(counter++).toString().padStart(12, "0")}`,
  });
}

if (typeof Element !== "undefined") {
  Element.prototype.animate = function animateStub() {
    const anim: Record<string, unknown> = {
      onfinish: null,
      oncancel: null,
      cancel() {},
      finish() {},
      play() {},
      pause() {},
      reverse() {},
      persist() {},
      updatePlaybackRate() {},
      currentTime: 0,
      startTime: 0,
      playbackRate: 1,
      effect: null,
      playState: "finished",
      finished: Promise.resolve(),
    };
    return anim as unknown as Animation;
  } as typeof Element.prototype.animate;
}

if (typeof window !== "undefined" && !window.matchMedia) {
  window.matchMedia = ((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    addListener: () => undefined,
    removeListener: () => undefined,
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
}
