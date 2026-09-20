import { describe, it, expect } from "vitest";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const here = dirname(fileURLToPath(import.meta.url));
const distDir = resolve(here, "../../frontend/dist/assets");

const BUDGET_JS_GZIP_BYTES = 250 * 1024;
const BUDGET_CSS_GZIP_BYTES = 75 * 1024;
/** Views deferred by `frontend/src/lib/lazyViews.ts`: downloaded only when the user opens them. */
const DEFERRED_VIEW_PREFIXES = [
  "About-",
  "Backups-",
  "Catalog-",
  "Drivers-",
  "Journal-",
  "Settings-",
];
/** Extended locales loaded on demand by `frontend/src/lib/i18n/index.ts`; `en` and `es` are inlined. */
const DEFERRED_LOCALE_PREFIXES = ["pt-BR-", "de-", "fr-", "ja-", "ru-", "zh-CN-"];
const KNOWN_CHUNK_PREFIXES = [
  "index-",
  "image-",
  "app-",
  "window-",
  "icons-",
  "svelte-",
  "tauri-",
  "vendor-",
  "stores-",
  "FlyoutShell-",
  ...DEFERRED_VIEW_PREFIXES,
  ...DEFERRED_LOCALE_PREFIXES,
];
const STEALTH_CHUNK_MIN_BYTES = 1024;

function gzipBytes(path: string): number {
  return gzipSync(readFileSync(path)).length;
}

function fmt(bytes: number): string {
  return `${(bytes / 1024).toFixed(1)} KB`;
}

describe("bundle budget preconditions", () => {
  it.runIf(process.env.CI)("frontend/dist exists on CI — the budget suite must not silently skip", () => {
    expect(
      existsSync(distDir),
      `${distDir} is missing — run the Vite build BEFORE vitest in CI or the budget is never enforced`,
    ).toBe(true);
  });
});

describe.skipIf(!existsSync(distDir))(
  "bundle size budget (post-build, gzip-measured)",
  () => {
    it("the biggest index-*.js chunk stays under the gzip budget", () => {
      type Chunk = { name: string; gzip: number };
      const candidates: Chunk[] = readdirSync(distDir)
        .filter((f: string) => f.startsWith("index-") && f.endsWith(".js"))
        .map(
          (name: string): Chunk => ({
            name,
            gzip: gzipBytes(resolve(distDir, name)),
          }),
        )
        .sort((a: Chunk, b: Chunk) => b.gzip - a.gzip);

      expect(candidates.length, `no index-*.js in ${distDir}`).toBeGreaterThan(0);

      const biggest = candidates[0];
      expect(
        biggest.gzip,
        `${biggest.name} = ${fmt(biggest.gzip)} gzip exceeds the ${fmt(BUDGET_JS_GZIP_BYTES)} budget`,
      ).toBeLessThan(BUDGET_JS_GZIP_BYTES);
    });

    it("every index-*.css bundle stays under the gzip budget", () => {
      type Chunk = { name: string; gzip: number };
      const candidates: Chunk[] = readdirSync(distDir)
        .filter((f: string) => f.startsWith("index-") && f.endsWith(".css"))
        .map(
          (name: string): Chunk => ({
            name,
            gzip: gzipBytes(resolve(distDir, name)),
          }),
        );

      expect(candidates.length, `no index-*.css in ${distDir}`).toBeGreaterThan(0);

      for (const c of candidates) {
        expect(
          c.gzip,
          `${c.name} = ${fmt(c.gzip)} gzip exceeds the ${fmt(BUDGET_CSS_GZIP_BYTES)} budget`,
        ).toBeLessThan(BUDGET_CSS_GZIP_BYTES);
      }
    });

    it("no stealth JS chunk lands outside the known emit prefixes", () => {
      const surprises: string[] = readdirSync(distDir)
        .filter((f: string) => f.endsWith(".js"))
        .filter((f: string) => gzipBytes(resolve(distDir, f)) > STEALTH_CHUNK_MIN_BYTES)
        .filter((f: string) => !KNOWN_CHUNK_PREFIXES.some((p) => f.startsWith(p)));

      expect(
        surprises,
        `unknown >${fmt(STEALTH_CHUNK_MIN_BYTES)} JS chunks: ${surprises.join(", ")} — add the prefix to KNOWN_CHUNK_PREFIXES intentionally after reviewing what's inside`,
      ).toEqual([]);
    });

    it("the startup JS payload stays under the budget for every shipped language", () => {
      const jsFiles: string[] = readdirSync(distDir).filter((f: string) => f.endsWith(".js"));
      const isDeferred = (name: string): boolean =>
        [...DEFERRED_VIEW_PREFIXES, ...DEFERRED_LOCALE_PREFIXES].some((p) => name.startsWith(p));

      const startupBytes = jsFiles
        .filter((f: string) => !isDeferred(f))
        .reduce((total: number, f: string) => total + gzipBytes(resolve(distDir, f)), 0);

      const localeBytes = jsFiles
        .filter((f: string) => DEFERRED_LOCALE_PREFIXES.some((p) => f.startsWith(p)))
        .map((f: string) => ({ name: f, gzip: gzipBytes(resolve(distDir, f)) }))
        .sort((a, b) => b.gzip - a.gzip);

      expect(localeBytes.length, "no deferred locale chunk found; locale splitting regressed").toBe(
        DEFERRED_LOCALE_PREFIXES.length,
      );

      // `en` and `es` are inlined, so their startup cost is already inside the non-deferred total.
      // Every other language adds exactly one locale chunk, so the worst language sets the ceiling.
      const worstLocale = localeBytes[0];
      const worstStartup = startupBytes + worstLocale.gzip;

      expect(
        worstStartup,
        `startup JS ${fmt(startupBytes)} + ${worstLocale.name} ${fmt(worstLocale.gzip)} = ${fmt(worstStartup)} gzip exceeds the ${fmt(BUDGET_JS_GZIP_BYTES)} budget`,
      ).toBeLessThan(BUDGET_JS_GZIP_BYTES);
    });
  },
);
