import { describe, expect, it } from "vitest";
import { autostartCheckboxValue, classifyAutostartReadback } from "@/views/Settings.svelte";

/** 10.13 — the startup entry is shown as the operating system reports it.
 *  A read that returns no boolean is unknown, not "off", and a difference between the requested and
 *  the observed value stays visible instead of being written away. */
describe("Settings — Windows startup entry state", () => {
  it("keeps an unreadable answer apart from a read 'off'", () => {
    expect(classifyAutostartReadback(false, null)).toEqual({ kind: "observed", enabled: false });
    expect(classifyAutostartReadback(undefined, null)).toEqual({ kind: "unknown", reason: "unreadable" });
    expect(classifyAutostartReadback(null, true)).toEqual({ kind: "unknown", reason: "unreadable" });
    expect(classifyAutostartReadback("true", true)).toEqual({ kind: "unknown", reason: "unreadable" });
  });

  it("reports a difference between the requested and the observed value", () => {
    expect(classifyAutostartReadback(false, true)).toEqual({ kind: "mismatch", observed: false, requested: true });
    expect(classifyAutostartReadback(true, false)).toEqual({ kind: "mismatch", observed: true, requested: false });
    expect(classifyAutostartReadback(true, true)).toEqual({ kind: "observed", enabled: true });
  });

  it("shows the observed value, not the requested one", () => {
    expect(autostartCheckboxValue({ kind: "observed", enabled: true }, false)).toBe(true);
    expect(autostartCheckboxValue({ kind: "observed", enabled: false }, true)).toBe(false);
    expect(autostartCheckboxValue({ kind: "mismatch", observed: false, requested: true }, true)).toBe(false);
  });

  it("falls back to the stored preference only while no read-back exists", () => {
    expect(autostartCheckboxValue({ kind: "unknown", reason: "not-read" }, true)).toBe(true);
    expect(autostartCheckboxValue({ kind: "unknown", reason: "unreadable" }, false)).toBe(false);
    expect(autostartCheckboxValue({ kind: "error", error: "plugin missing" }, true)).toBe(true);
  });
});
