import { readFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";

const root = process.cwd();
const generatedConfigPath = path.join(root, "target", "channels", "nexus", "config", "tauri.conf.json");
const generatedCapabilityPath = path.join(root, "target", "channels", "nexus", "config", "default.capability.json");
const frontendEnvPath = path.join(root, "frontend", ".env.nexus");
const libPath = path.join(root, "src-tauri", "src", "lib.rs");

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

const configText = await readFile(generatedConfigPath, "utf8");
const config = JSON.parse(configText);
assert(config.productName === "NexusBuild-DLSSync", "Nexus config must use the NexusBuild-DLSSync product identity");
assert(config.plugins?.updater === null, "Nexus overlay must explicitly delete the Standard updater configuration");
assert(!configText.includes("latest.json"), "Nexus config must not contain latest.json");
assert(config.build?.frontendDist === "../frontend/dist-nexus", "Nexus frontend output must be isolated in dist-nexus");

const capabilityText = await readFile(generatedCapabilityPath, "utf8");
const capability = JSON.parse(capabilityText);
assert(!capability.permissions.includes("updater:default"), "Nexus capability must remove updater:default");
assert(
  JSON.stringify(config.app?.security?.capabilities) === JSON.stringify([capability]),
  "Nexus config must select the derived updater-free capability inline",
);

const frontendEnv = await readFile(frontendEnvPath, "utf8");
assert(frontendEnv.includes("VITE_DLSSYNC_DISTRIBUTION=nexus"), "frontend/.env.nexus must set the nexus distribution");

const lib = await readFile(libPath, "utf8");
assert(lib.includes('#[cfg(feature = "standard")]'), "Tauri updater plugin must be standard-feature-only");
const cargo = await readFile(path.join(root, "src-tauri", "Cargo.toml"), "utf8");
assert(cargo.includes('standard = ["dep:tauri-plugin-updater"]'), "standard feature must own updater dependency");
assert(cargo.includes('tauri-plugin-updater = { version = "2", optional = true }'), "updater dependency must be optional");

console.log("Nexus build strip checks passed");
