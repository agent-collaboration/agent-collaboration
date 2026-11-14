import assert from "node:assert/strict";
import { mkdtempSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { once } from "node:events";
import { test } from "node:test";
import { listenForHeldRelease } from "../../../fixtures/held-release-control.ts";
import { releaseHeldWork } from "./release-sentinel.ts";

test("the private socket releases its owner once and refuses foreign or duplicate releases", async (): Promise<void> => {
  const root = mkdtempSync(join(tmpdir(), "release-control-"));
  const socketPath = join(root, "release");
  let held = true;
  let releases = 0;
  const replies: string[] = [];
  const server = await listenForHeldRelease({
    socketPath,
    releaseOwner(ownerId): boolean {
      if (ownerId !== "session-owner" || !held) return false;
      held = false;
      releases++;
      return true;
    },
    recordReply(reply): void {
      replies.push(reply.kind);
    },
  });
  try {
    assert.equal(statSync(socketPath).mode & 0o777, 0o600);
    await assert.rejects(releaseHeldWork(socketPath, "foreign"), /refused/);
    assert.equal(releases, 0);
    await releaseHeldWork(socketPath, "session-owner");
    assert.equal(releases, 1);
    await assert.rejects(releaseHeldWork(socketPath, "session-owner"), /refused/);
    assert.equal(releases, 1);
    assert.deepEqual(replies, ["refused", "released", "refused"]);
  } finally {
    server.close();
    await once(server, "close");
    rmSync(root, { recursive: true, force: true });
  }
});
