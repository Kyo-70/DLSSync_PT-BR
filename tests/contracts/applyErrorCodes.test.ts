import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";
import { classifyApplyError, ERROR_CLASS_LABEL, normalizeErrorClass } from "@/lib/applyErrorClass";

describe("backend apply errors", () => {
  it("translates every emitted stable code in every shipped locale", () => {
    const rust = readFileSync(resolve("../src-tauri/src/commands/apply.rs"), "utf8");
    const emitted = [...rust.matchAll(/Some\("([a-z_]+)"\)/g)].map(match => match[1]);
    for (const code of emitted) expect(Object.keys(ERROR_CLASS_LABEL)).toContain(code);
    const folder = resolve("src/lib/i18n/locales");
    for (const file of readdirSync(folder).filter(file => file.endsWith(".json") && !file.startsWith("_"))) {
      const catalog = JSON.parse(readFileSync(resolve(folder, file), "utf8"));
      for (const code of Object.keys(ERROR_CLASS_LABEL)) {
        for (const field of ["label", "short", "hint"]) expect(catalog.errorClass[code]?.[field], `${file}: ${code}.${field}`).toBeTruthy();
      }
    }
  });
  it("does not offer retry for a Streamline preference block", () => {
    expect(classifyApplyError("sl.dlss_g.dll requires matching sl.interposer.dll", "streamline_locked").retryable).toBe(false);
    expect(classifyApplyError("minimum driver required", "driver_too_old").retryable).toBe(false);
    expect(normalizeErrorClass("unknown_future_backend_code")).toBe("other");
  });
});
