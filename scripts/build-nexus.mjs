import { spawn } from "node:child_process";
import { access, mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";

async function readJson(file) {
  return JSON.parse(await readFile(file, "utf8"));
}

export function stripUpdaterCapability(capability) {
  const { $schema: _schema, ...portableCapability } = capability;
  return {
    ...portableCapability,
    permissions: capability.permissions.filter((permission) => permission !== "updater:default"),
  };
}

export function createNexusConfig(base, capability) {
  const config = structuredClone(base);
  config.productName = "NexusBuild-DLSSync";
  config.build = {
    ...config.build,
    beforeBuildCommand:
      "pnpm --filter dlssync-frontend exec vite build --mode nexus --outDir dist-nexus --emptyOutDir",
    frontendDist: "../frontend/dist-nexus",
  };
  config.app = {
    ...config.app,
    security: {
      ...config.app?.security,
      capabilities: [capability],
    },
  };
  config.bundle = {
    ...config.bundle,
    shortDescription: "DLSSync Nexus build with manual network actions",
    longDescription:
      "NexusBuild-DLSSync keeps local DLL update and backup workflows while removing the app updater. Network actions require an explicit user action.",
  };
  // RFC 7396 merge-patch deletion: omission would retain the Standard endpoint from the base config.
  config.plugins = { ...config.plugins, updater: null };
  return config;
}

export async function prepareNexusBuild(root = process.cwd()) {
  const configDir = path.join(root, "target", "channels", "nexus", "config");
  const generatedConfigPath = path.join(configDir, "tauri.conf.json");
  const generatedCapabilityPath = path.join(configDir, "default.capability.json");
  const tauriConfigPath = path.join(root, "src-tauri", "tauri.conf.json");
  const capabilityPath = path.join(root, "src-tauri", "capabilities", "default.json");
  const capability = stripUpdaterCapability(await readJson(capabilityPath));
  const config = createNexusConfig(await readJson(tauriConfigPath), capability);
  await mkdir(configDir, { recursive: true });
  await writeFile(generatedConfigPath, `${JSON.stringify(config, null, 2)}\n`);
  await writeFile(generatedCapabilityPath, `${JSON.stringify(capability, null, 2)}\n`);
  return {
    config,
    capability,
    generatedConfigPath,
    generatedCapabilityPath,
    cargoTargetDir: path.join(root, "target", "channels", "nexus"),
  };
}

export async function packageManagerCommand(env = process.env) {
  if (env.npm_execpath && /\.(?:c?js|mjs)$/i.test(env.npm_execpath)) {
    await access(env.npm_execpath);
    return { command: process.execPath, prefix: [env.npm_execpath] };
  }
  if (process.platform !== "win32") return { command: "pnpm", prefix: [] };
  const pathValue = Object.entries(env).find(([key]) => key.toLowerCase() === "path")?.[1] ?? "";
  for (const directory of pathValue.split(path.delimiter).filter(Boolean)) {
    for (const relative of ["node_modules/pnpm/bin/pnpm.mjs", "node_modules/pnpm/bin/pnpm.cjs", "node_modules/corepack/dist/pnpm.js"]) {
      const entry = path.join(directory, relative);
      try {
        await access(entry);
        return { command: process.execPath, prefix: [entry] };
      } catch { /* Try the next installed package-manager entry. */ }
    }
  }
  throw new Error("Cannot locate the pnpm JavaScript entry point on PATH");
}

export function run(command, args, options) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd: options.cwd,
      env: options.env,
      stdio: "inherit",
      shell: false,
    });
    child.on("exit", (code) => {
      if (code === 0) resolve();
      else reject(new Error(`${command} ${args.join(" ")} exited ${code}`));
    });
    child.on("error", reject);
  });
}

export async function main(argv = process.argv.slice(2), root = process.cwd()) {
  const prepared = await prepareNexusBuild(root);
  if (argv.includes("--prepare-only")) {
    console.log(`Prepared Nexus build config: ${prepared.generatedConfigPath}`);
    console.log(`Prepared Nexus capability: ${prepared.generatedCapabilityPath}`);
    return;
  }

  const env = {
    ...process.env,
    CARGO_TARGET_DIR: prepared.cargoTargetDir,
    VITE_DLSSYNC_DISTRIBUTION: "nexus",
  };
  delete env.TAURI_SIGNING_PRIVATE_KEY;
  delete env.TAURI_SIGNING_PRIVATE_KEY_PATH;
  delete env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD;

  // Invoke the pinned CLI directly. Package-manager exec wrappers can consume the Cargo separator.
  const tauriCli = path.join(root, "node_modules", "@tauri-apps", "cli", "tauri.js");
  await access(tauriCli);
  await run(
    process.execPath,
    [
      tauriCli,
      "build",
      "--config",
      prepared.generatedConfigPath,
      "--target",
      "x86_64-pc-windows-msvc",
      "--features",
      "nexus",
      "--bundles",
      "nsis,msi",
      "--",
      "--no-default-features",
      "--locked",
    ],
    { cwd: root, env },
  );
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  await main();
}
