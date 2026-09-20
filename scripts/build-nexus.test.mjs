import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { packageManagerCommand, run, stripUpdaterCapability, createNexusConfig } from "./build-nexus.mjs";

const root = path.resolve(import.meta.dirname, "..");
const scriptPath = path.join(root, "scripts", "build-nexus.mjs");

test("the installed package manager launches without a Windows command shell", async () => {
  const manager = await packageManagerCommand();
  await run(manager.command, [...manager.prefix, "--version"], { cwd: root, env: process.env });
});

test("process launch preserves paths with spaces as one argument", async () => {
  await run(process.execPath, ["-e", "if (process.argv[1] !== 'config path with spaces') process.exit(1)", "config path with spaces"], { cwd: root, env: process.env });
});

test("Nexus build uses derived config and isolated outputs without writing tracked sources", async () => {
  const source = await readFile(scriptPath, "utf8");
  assert.doesNotMatch(source, /writeFile\(tauriConfigPath/);
  assert.doesNotMatch(source, /writeFile\(capabilityPath/);
  assert.doesNotMatch(source, /originalConfig|originalCapability|finally\s*\{/);
  assert.match(source, /--config/);
  assert.match(source, /CARGO_TARGET_DIR/);
  assert.match(source, /dist-nexus/);
  const separator = source.indexOf('"--",');
  assert.ok(separator > 0);
  assert.ok(source.indexOf('"--no-default-features"') > separator);
  assert.match(source, /process\.execPath/);
});

test("Nexus removes only its updater permission and leaves the Standard owner unchanged", async () => {
  const original = JSON.parse(await readFile(path.join(root, "src-tauri", "capabilities", "default.json"), "utf8"));
  const before = JSON.stringify(original);
  const nexus = stripUpdaterCapability(original);
  assert.equal(JSON.stringify(original), before);
  assert.ok(original.permissions.includes("updater:default"));
  assert.deepEqual(nexus.permissions, original.permissions.filter(permission => permission !== "updater:default"));
  assert.deepEqual(nexus.windows, original.windows);
});

test("Nexus overlay explicitly removes the Standard updater during merge-patch", () => {
  const base = { plugins: { updater: { endpoints: ["https://example.test/latest.json"] }, shell: { open: true } } };
  const nexus = createNexusConfig(base, { permissions: ["core:default"] });
  assert.equal(nexus.plugins.updater, null);
  assert.deepEqual(nexus.plugins.shell, { open: true });
  assert.deepEqual(base.plugins.updater.endpoints, ["https://example.test/latest.json"]);
  assert.ok(!JSON.stringify(nexus).includes("latest.json"));
});
