import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

const root = path.resolve(import.meta.dirname, "..");
const workflowPath = path.join(root, ".github", "workflows", "release.yml");

test("release verification binds the feed and checksums to exact Standard bytes", async () => {
  const { releaseAssetNames, writeChecksums, verifyReleaseDirectory } = await import("./release-safety.mjs");
  const version = "9.8.7";
  const temp = await mkdtemp(path.join(os.tmpdir(), "dlssync-release-exact-"));
  try {
    for (const name of releaseAssetNames(version)) await writeFile(path.join(temp, name), name);
    const installer = `DLSSync_${version}_x64-setup.exe`;
    const feed = { version, platforms: { "windows-x86_64": { signature: `${installer}.sig`, url: `https://example.test/${installer}` } } };
    await writeFile(path.join(temp, "latest.json"), JSON.stringify(feed));
    await writeChecksums(temp, path.join(temp, "SHA256SUMS.txt"), version);
    await verifyReleaseDirectory(temp, version);
    await writeFile(path.join(temp, installer), "changed after checksums");
    await assert.rejects(() => verifyReleaseDirectory(temp, version), /SHA-256 mismatch/);
    feed.platforms["windows-x86_64"].url = "https://example.test/wrong-setup.exe";
    await writeFile(path.join(temp, "latest.json"), JSON.stringify(feed));
    await assert.rejects(() => verifyReleaseDirectory(temp, version), /exact Standard installer/);
    feed.platforms["windows-x86_64"].url = `https://example.test/${installer}`;
    feed.platforms["windows-x86_64"].signature = "different";
    await writeFile(path.join(temp, "latest.json"), JSON.stringify(feed));
    await assert.rejects(() => verifyReleaseDirectory(temp, version), /signature differs/);
  } finally {
    await rm(temp, { recursive: true, force: true });
  }
});

test("publication never deletes a release before recreation", async () => {
  const workflow = await readFile(workflowPath, "utf8");
  assert.doesNotMatch(workflow, /gh\s+release\s+delete/i);
  assert.doesNotMatch(workflow, /Delete existing release/i);
});

test("publication retries reuse retained files without compiling or signing again", async () => {
  const workflow = await readFile(workflowPath, "utf8");
  const publish = workflow.split(/\r?\n  publish:\r?\n/)[1];
  assert.ok(publish, "a separate publication job must exist");
  assert.match(publish, /dlssync-release-ready/);
  assert.match(publish, /Release identity mismatch/);
  assert.match(publish, /release-safety\.mjs verify/);
  assert.doesNotMatch(publish, /tauri build|signer sign|build-nexus\.mjs/);
});

test("publication requires one verified signature state and never fakes one", async () => {
  const workflow = await readFile(workflowPath, "utf8");
  // Exactly one lane may produce the release input, and each lane must read the real
  // signature status instead of assuming it.
  assert.match(workflow, /sign-windows:[\s\S]*?if:\s*vars\.SIGNPATH_ENABLED\s*==\s*'true'/);
  assert.match(workflow, /stage-unsigned:[\s\S]*?if:\s*vars\.SIGNPATH_ENABLED\s*!=\s*'true'/);
  assert.match(workflow, /verify-signatures:[\s\S]*?if:\s*needs\.sign-windows\.result\s*==\s*'success'/);
  assert.match(workflow, /needs\.verify-signatures\.result\s*==\s*'success'\s*\|\|\s*needs\.stage-unsigned\.result\s*==\s*'success'/);
  assert.match(workflow, /publish:[\s\S]*?if:\s*>-[\s\S]*?always\(\)[\s\S]*?needs\.prepare-release\.result\s*==\s*'success'/);
  assert.match(workflow, /needs\.sign-windows\.result\s*!=\s*'failure'/);
  assert.match(workflow, /needs\.verify-signatures\.result\s*!=\s*'failure'/);
  assert.match(workflow, /needs\.stage-unsigned\.result\s*!=\s*'failure'/);
  // The signed lane accepts nothing but a valid signature.
  assert.match(workflow, /if \(\$signature\.Status -ne 'Valid'\)/);
  // The unsigned lane refuses any file that unexpectedly carries a signature.
  assert.match(workflow, /if \(\$status -ne 'NotSigned'\)/);
  assert.match(workflow, /Unsigned lane refuses exact artifact/);
  // Publication and release notes require a recorded state that one of those lanes verified.
  assert.match(workflow, /AUTHENTICODE-STATE\.json/);
  assert.match(workflow, /Refusing publication: recorded Authenticode state/);
  assert.match(workflow, /Refusing release notes without a verified Authenticode state/);
  assert.match(workflow, /not Authenticode-signed/);
  assert.match(workflow, /name:\s*dlssync-windows-release-input/);
  assert.match(workflow, /Authenticode signature missing or invalid for exact artifact/);
  assert.match(workflow, /Get-AuthenticodeSignature/);
  assert.match(workflow, /Create Tauri updater signature for final Standard NSIS bytes/);
  assert.match(workflow, /Tauri updater signature missing for exact artifact/);
  assert.match(workflow, /tags:\s*\r?\n\s*- "v1\.7\.0"/);
  assert.match(workflow, /Build Standard in isolated outputs/);
  assert.match(workflow, /Build NexusBuild in isolated outputs/);
  assert.doesNotMatch(workflow, /NexusMods-Only|nexusmods-only/i);
});

test("no release lane weakens Windows protections or vendor DLL publisher trust", async () => {
  const workflow = await readFile(workflowPath, "utf8");
  assert.doesNotMatch(workflow, /Set-MpPreference|Add-MpPreference|DisableRealtimeMonitoring/i);
  assert.doesNotMatch(workflow, /SmartScreen[^.\n]*(disable|bypass|off)/i);
  assert.doesNotMatch(workflow, /New-SelfSignedCertificate|makecert|signtool\s+sign/i);
  // The apply path's publisher requirement is source behavior, not a release-time toggle.
  const applyPolicy = await readFile(path.join(root, "crates", "dlssync-application", "src", "execution.rs"), "utf8");
  assert.match(applyPolicy, /publisher/i);
});

test("latest.json rejects multiple exact Standard installer candidates", async () => {
  const { selectExactStandardInstaller } = await import("./release-safety.mjs");
  const temp = await mkdtemp(path.join(os.tmpdir(), "dlssync-release-test-"));
  try {
    const name = "DLSSync_1.7.0_x64-setup.exe";
    await mkdir(path.join(temp, "a"));
    await mkdir(path.join(temp, "b"));
    await writeFile(path.join(temp, "a", name), "one");
    await writeFile(path.join(temp, "b", name), "two");
    await assert.rejects(() => selectExactStandardInstaller(temp, "1.7.0"), /exactly one.*found 2/i);
  } finally {
    await rm(temp, { recursive: true, force: true });
  }
});

test("latest.json rejects a NexusBuild installer", async () => {
  const { selectExactStandardInstaller } = await import("./release-safety.mjs");
  const temp = await mkdtemp(path.join(os.tmpdir(), "dlssync-release-test-"));
  try {
    await writeFile(path.join(temp, "NexusBuild-DLSSync_1.7.0_x64-setup.exe"), "nexus");
    await assert.rejects(() => selectExactStandardInstaller(temp, "1.7.0"), /NexusBuild|Standard installer/i);
  } finally {
    await rm(temp, { recursive: true, force: true });
  }
});

test("latest.json rejects an empty Tauri updater signature", async () => {
  const { readNonEmptyUpdaterSignature } = await import("./release-safety.mjs");
  const temp = await mkdtemp(path.join(os.tmpdir(), "dlssync-release-test-"));
  try {
    const signature = path.join(temp, "DLSSync_1.7.0_x64-setup.exe.sig");
    await writeFile(signature, "   \n");
    await assert.rejects(() => readNonEmptyUpdaterSignature(signature), /empty.*Tauri updater signature/i);
  } finally {
    await rm(temp, { recursive: true, force: true });
  }
});
