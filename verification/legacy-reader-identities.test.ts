import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmod, copyFile, mkdtemp, readFile, realpath, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { SealedLegacyCopy } from "./inputs/sealed-legacy-copy.ts";
import { readLegacyReaderIdentities } from "./inputs/legacy-reader-identities.ts";

test("legacy reader lookup keeps the original opaque key and exact public identity", async () => {
  const root = await realpath(await mkdtemp(join(tmpdir(), "2026-10-reader-identities-")));
  try {
    const fixture = fileURLToPath(
      new URL("../crates/board-legacy-upgrade/tests/fixtures/role-history.sqlite", import.meta.url),
    );
    const path = join(root, "role-history.sqlite");
    await copyFile(fixture, path);
    await chmod(path, 0o400);
    const copy = await SealedLegacyCopy.qualify({
      path,
      copyRoot: root,
      sha256: createHash("sha256")
        .update(await readFile(fixture))
        .digest("hex"),
    });
    const identities = await readLegacyReaderIdentities(copy);
    assert.deepEqual(
      identities.get("session:550e8400-e29b-41d4-a716-446655440000:codex-local:actor-a"),
      {
        kind: "session",
        session: {
          endpoint: {
            serviceId: "550e8400-e29b-41d4-a716-446655440000",
            endpointId: "codex-local",
          },
          sessionId: "actor-a",
        },
      },
    );
    assert.deepEqual(identities.get("human:legacy-owner"), {
      kind: "human",
      humanId: "legacy-owner",
    });
    await copy.verify();
  } finally {
    await rm(root, { recursive: true });
  }
});
