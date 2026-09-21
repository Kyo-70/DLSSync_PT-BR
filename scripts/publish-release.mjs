import { createHash } from "node:crypto";
import { readdir, readFile, stat } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";
import { releaseAssetNames } from "./release-safety.mjs";

function parseArgs(argv) {
  const values = {};
  for (let index = 0; index < argv.length; index += 2) {
    values[argv[index]?.replace(/^--/, "")] = argv[index + 1];
  }
  return values;
}

function apiHeaders(token, extra = {}) {
  return {
    Accept: "application/vnd.github+json",
    Authorization: `Bearer ${token}`,
    "X-GitHub-Api-Version": "2022-11-28",
    ...extra,
  };
}

async function requestJson(url, token, options = {}, allowedStatuses = []) {
  const response = await fetch(url, { ...options, headers: apiHeaders(token, options.headers) });
  if (allowedStatuses.includes(response.status)) return { response, body: null };
  if (!response.ok) {
    throw new Error(`GitHub API ${options.method ?? "GET"} ${url} failed: ${response.status} ${await response.text()}`);
  }
  const text = await response.text();
  return { response, body: text ? JSON.parse(text) : null };
}

async function getPublishedRelease(repo, tag, token) {
  const url = `https://api.github.com/repos/${repo}/releases/tags/${encodeURIComponent(tag)}`;
  const { response, body } = await requestJson(url, token, {}, [404]);
  return response.status === 404 ? null : body;
}

export async function getRelease(repo, tag, token) {
  const published = await getPublishedRelease(repo, tag, token);
  if (published) return published;

  for (let page = 1; page <= 10; page += 1) {
    const releases = (
      await requestJson(
        `https://api.github.com/repos/${repo}/releases?per_page=100&page=${page}`,
        token,
      )
    ).body;
    const match = releases.find((release) => release.tag_name === tag);
    if (match) return match;
    if (releases.length < 100) break;
  }
  return null;
}

async function resolveTagCommit(repo, tag, token) {
  let object = (
    await requestJson(`https://api.github.com/repos/${repo}/git/ref/tags/${encodeURIComponent(tag)}`, token)
  ).body.object;
  for (let depth = 0; object.type === "tag" && depth < 5; depth += 1) {
    object = (await requestJson(object.url, token)).body.object;
  }
  if (object.type !== "commit") throw new Error(`Tag '${tag}' did not resolve to a commit`);
  return object.sha;
}

async function sha256Buffer(value) {
  return createHash("sha256").update(value).digest("hex");
}

async function localAssets(directory, version) {
  const expected = releaseAssetNames(version).sort();
  const actual = (await readdir(directory, { withFileTypes: true }))
    .filter((entry) => entry.isFile())
    .map((entry) => entry.name)
    .sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`Refusing publication with unexpected inventory: ${actual.join(", ")}`);
  }
  const assets = new Map();
  for (const name of expected) {
    const file = path.join(directory, name);
    if ((await stat(file)).size === 0) throw new Error(`Refusing empty release asset '${name}'`);
    const bytes = await readFile(file);
    assets.set(name, { bytes, hash: await sha256Buffer(bytes) });
  }
  return assets;
}

async function remoteAssetHash(asset, token) {
  const response = await fetch(asset.url, {
    headers: apiHeaders(token, { Accept: "application/octet-stream" }),
    redirect: "follow",
  });
  if (!response.ok) throw new Error(`Could not download existing asset '${asset.name}': ${response.status}`);
  return sha256Buffer(Buffer.from(await response.arrayBuffer()));
}

async function validateRemoteAssets(release, expected, token, allowMissing) {
  const remote = new Map();
  for (const asset of release.assets ?? []) {
    if (remote.has(asset.name)) throw new Error(`Remote release contains duplicate asset '${asset.name}'`);
    if (!expected.has(asset.name)) throw new Error(`Remote release contains unexpected asset '${asset.name}'`);
    remote.set(asset.name, asset);
  }
  for (const [name, local] of expected) {
    const asset = remote.get(name);
    if (!asset) {
      if (allowMissing) continue;
      throw new Error(`Remote release is missing exact asset '${name}'`);
    }
    if (asset.size !== local.bytes.length) throw new Error(`Remote asset '${name}' has a conflicting size`);
    if ((await remoteAssetHash(asset, token)) !== local.hash) {
      throw new Error(`Remote asset '${name}' has conflicting bytes`);
    }
  }
  return remote;
}

async function createDraft(repo, tag, title, notes, commit, token) {
  const url = `https://api.github.com/repos/${repo}/releases`;
  const payload = { tag_name: tag, target_commitish: commit, name: title, body: notes, draft: true, prerelease: false };
  const response = await fetch(url, {
    method: "POST",
    headers: apiHeaders(token, { "Content-Type": "application/json" }),
    body: JSON.stringify(payload),
  });
  if (response.status === 422) return null;
  if (!response.ok) throw new Error(`Could not create release draft: ${response.status} ${await response.text()}`);
  return response.json();
}

async function uploadMissing(repo, release, expected, remote, token) {
  for (const [name, local] of expected) {
    if (remote.has(name)) continue;
    const url = `https://uploads.github.com/repos/${repo}/releases/${release.id}/assets?name=${encodeURIComponent(name)}`;
    const response = await fetch(url, {
      method: "POST",
      headers: apiHeaders(token, { "Content-Type": "application/octet-stream" }),
      body: local.bytes,
    });
    if (!response.ok) throw new Error(`Upload of '${name}' failed safely: ${response.status} ${await response.text()}`);
  }
}

async function publishDraft(repo, release, title, notes, token) {
  const url = `https://api.github.com/repos/${repo}/releases/${release.id}`;
  await requestJson(url, token, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name: title, body: notes, draft: false, prerelease: false, make_latest: "true" }),
  });
}

export async function publishRelease({ assetsDir, commit, notesPath, repo, tag, title, token, version }) {
  const expected = await localAssets(assetsDir, version);
  const notes = await readFile(notesPath, "utf8");
  const remoteCommit = await resolveTagCommit(repo, tag, token);
  if (remoteCommit !== commit) {
    throw new Error(`Tag '${tag}' resolves to ${remoteCommit}, not approved commit ${commit}`);
  }

  let release = await getRelease(repo, tag, token);
  if (!release) {
    release = await createDraft(repo, tag, title, notes, commit, token);
    if (!release) release = await getRelease(repo, tag, token);
    if (!release) throw new Error(`Release creation raced, but '${tag}' is still unavailable`);
  }

  if (release.tag_name !== tag || release.name !== title || release.body !== notes || release.prerelease) {
    throw new Error(`Existing release '${tag}' metadata is not safe to update automatically`);
  }

  if (!release.draft) {
    await validateRemoteAssets(release, expected, token, false);
    console.log(`Published release '${tag}' already matches exact approved assets; no mutation needed`);
    return;
  }

  const remote = await validateRemoteAssets(release, expected, token, true);
  await uploadMissing(repo, release, expected, remote, token);
  release = await getRelease(repo, tag, token);
  if (!release?.draft || release.id == null) throw new Error(`Release '${tag}' changed state during upload`);
  await validateRemoteAssets(release, expected, token, false);
  if ((await resolveTagCommit(repo, tag, token)) !== commit) throw new Error(`Tag '${tag}' changed before publication`);
  await publishDraft(repo, release, title, notes, token);
  console.log(`Published verified draft '${tag}' in place`);
}

async function main(argv = process.argv.slice(2)) {
  const args = parseArgs(argv);
  const token = process.env.GH_TOKEN;
  if (!token) throw new Error("GH_TOKEN is required");
  await publishRelease({
    assetsDir: args.dir,
    commit: args.commit,
    notesPath: args.notes,
    repo: args.repository,
    tag: args.tag,
    title: args.title,
    token,
    version: args.version,
  });
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  await main();
}
