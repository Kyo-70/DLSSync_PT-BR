import assert from "node:assert/strict";
import test from "node:test";
import { getRelease } from "./publish-release.mjs";

test("draft releases are found through the release list when the tag endpoint returns 404", async () => {
  const originalFetch = globalThis.fetch;
  const calls = [];
  globalThis.fetch = async (url) => {
    calls.push(String(url));
    if (String(url).includes("/releases/tags/")) {
      return new Response("", { status: 404 });
    }
    return Response.json([{ id: 17, tag_name: "v1.7.0", draft: true }]);
  };

  try {
    const release = await getRelease("owner/repo", "v1.7.0", "token");
    assert.equal(release.id, 17);
    assert.equal(release.draft, true);
    assert.equal(calls.length, 2);
  } finally {
    globalThis.fetch = originalFetch;
  }
});
