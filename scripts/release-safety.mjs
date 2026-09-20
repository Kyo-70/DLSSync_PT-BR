import { createHash } from "node:crypto";
import { readdir, readFile, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";

export function releaseAssetNames(version) {
  return [
    `DLSSync_${version}_x64-setup.exe`,
    `DLSSync_${version}_x64-setup.exe.sig`,
    `DLSSync_${version}_x64_en-US.msi`,
    `DLSSync_${version}_x64-portable.zip`,
    `NexusBuild-DLSSync_${version}_x64-setup.exe`,
    `NexusBuild-DLSSync_${version}_x64_en-US.msi`,
    `NexusBuild-DLSSync_${version}_x64-portable.zip`,
    "latest.json",
    "SHA256SUMS.txt",
  ];
}

async function walkFiles(root) {
  const files = [];
  async function visit(directory) {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const fullPath = path.join(directory, entry.name);
      if (entry.isDirectory()) await visit(fullPath);
      else if (entry.isFile()) files.push(fullPath);
    }
  }
  await visit(root);
  return files;
}

export async function selectExactArtifact(root, exactName) {
  const matches = (await walkFiles(root)).filter((file) => path.basename(file) === exactName);
  if (matches.length !== 1) {
    throw new Error(`Expected exactly one '${exactName}' artifact; found ${matches.length}`);
  }
  if ((await stat(matches[0])).size === 0) {
    throw new Error(`Exact artifact '${exactName}' is empty`);
  }
  return matches[0];
}

export async function selectExactStandardInstaller(root, version) {
  const expected = `DLSSync_${version}_x64-setup.exe`;
  const files = await walkFiles(root);
  const nexusInstallers = files.filter((file) => /^NexusBuild-.*-setup\.exe$/i.test(path.basename(file)));
  const matches = files.filter((file) => path.basename(file) === expected);
  if (matches.length === 0 && nexusInstallers.length > 0) {
    throw new Error(`Refusing NexusBuild installer for Standard updater feed; required '${expected}'`);
  }
  if (matches.length !== 1) {
    throw new Error(`Expected exactly one Standard installer '${expected}'; found ${matches.length}`);
  }
  if ((await stat(matches[0])).size === 0) {
    throw new Error(`Exact Standard installer '${expected}' is empty`);
  }
  return matches[0];
}

export async function readNonEmptyUpdaterSignature(signaturePath) {
  const signature = (await readFile(signaturePath, "utf8")).trim();
  if (!signature) {
    throw new Error(`Empty Tauri updater signature for exact artifact '${path.basename(signaturePath)}'`);
  }
  return signature;
}

async function sha256(file) {
  return createHash("sha256").update(await readFile(file)).digest("hex");
}

export async function writeLatestJson({ artifactRoot, notesPath, outputPath, pubDate, repository, tag, version }) {
  const installer = await selectExactStandardInstaller(artifactRoot, version);
  if (path.basename(installer).startsWith("NexusBuild-")) {
    throw new Error("NexusBuild artifacts cannot be used in latest.json");
  }
  const signaturePath = await selectExactArtifact(artifactRoot, `${path.basename(installer)}.sig`);
  const signature = await readNonEmptyUpdaterSignature(signaturePath);
  const latest = {
    version,
    notes: await readFile(notesPath, "utf8"),
    pub_date: pubDate,
    platforms: {
      "windows-x86_64": {
        signature,
        url: `https://github.com/${repository}/releases/download/${tag}/${path.basename(installer)}`,
      },
    },
  };
  await writeFile(outputPath, `${JSON.stringify(latest, null, 2)}\n`);
}

export async function writeChecksums(artifactRoot, outputPath, version) {
  const names = releaseAssetNames(version).filter((name) => name !== "SHA256SUMS.txt");
  const lines = [];
  for (const name of names) {
    const file = await selectExactArtifact(artifactRoot, name);
    lines.push(`${await sha256(file)}  ${name}`);
  }
  await writeFile(outputPath, `${lines.join("\n")}\n`);
}

export async function verifyReleaseDirectory(artifactRoot, version) {
  const expected = releaseAssetNames(version).sort();
  const actual = (await readdir(artifactRoot, { withFileTypes: true }))
    .filter((entry) => entry.isFile())
    .map((entry) => entry.name)
    .sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`Release inventory mismatch. Expected ${expected.join(", ")}; found ${actual.join(", ")}`);
  }
  for (const name of expected) await selectExactArtifact(artifactRoot, name);
  const latest = JSON.parse(await readFile(path.join(artifactRoot, "latest.json"), "utf8"));
  const platforms = Object.keys(latest.platforms ?? {});
  if (platforms.length !== 1 || platforms[0] !== "windows-x86_64") {
    throw new Error("latest.json must contain exactly the windows-x86_64 platform");
  }
  if (!latest.platforms["windows-x86_64"].signature?.trim()) {
    throw new Error("latest.json contains an empty Tauri updater signature");
  }
  if (/NexusBuild-/i.test(latest.platforms["windows-x86_64"].url)) {
    throw new Error("latest.json points to a NexusBuild artifact");
  }
  if (latest.version !== version) throw new Error("latest.json version differs from release version");
  const installerName = `DLSSync_${version}_x64-setup.exe`;
  const url = new URL(latest.platforms["windows-x86_64"].url);
  if (url.protocol !== "https:" || decodeURIComponent(url.pathname.split("/").at(-1)) !== installerName) {
    throw new Error("latest.json does not point to the exact Standard installer");
  }
  const signature = await readNonEmptyUpdaterSignature(path.join(artifactRoot, `${installerName}.sig`));
  if (latest.platforms["windows-x86_64"].signature.trim() !== signature) {
    throw new Error("latest.json signature differs from the Standard installer signature");
  }
  const checksums = await readFile(path.join(artifactRoot, "SHA256SUMS.txt"), "utf8");
  const entries = new Map();
  for (const line of checksums.trim().split(/\r?\n/)) {
    const match = /^([a-fA-F0-9]{64}) {2}([^/\\]+)$/.exec(line);
    if (!match || entries.has(match[2])) throw new Error("Invalid or duplicate checksum entry");
    entries.set(match[2], match[1].toLowerCase());
  }
  const hashedNames = expected.filter((name) => name !== "SHA256SUMS.txt");
  if (entries.size !== hashedNames.length) throw new Error("Checksum inventory mismatch");
  for (const name of hashedNames) {
    if (entries.get(name) !== await sha256(path.join(artifactRoot, name))) {
      throw new Error(`SHA-256 mismatch for exact release artifact '${name}'`);
    }
  }
}

function parseArgs(argv) {
  const [action, ...rest] = argv;
  const values = { action };
  for (let index = 0; index < rest.length; index += 2) {
    values[rest[index]?.replace(/^--/, "")] = rest[index + 1];
  }
  return values;
}

async function main(argv = process.argv.slice(2)) {
  const args = parseArgs(argv);
  if (args.action === "latest") {
    await writeLatestJson({
      artifactRoot: args.dir,
      notesPath: args.notes,
      outputPath: path.join(args.dir, "latest.json"),
      pubDate: args["pub-date"],
      repository: args.repository,
      tag: args.tag,
      version: args.version,
    });
    return;
  }
  if (args.action === "checksums") {
    await writeChecksums(args.dir, path.join(args.dir, "SHA256SUMS.txt"), args.version);
    return;
  }
  if (args.action === "verify") {
    await verifyReleaseDirectory(args.dir, args.version);
    return;
  }
  throw new Error("Usage: release-safety.mjs <latest|checksums|verify> [options]");
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  await main();
}
