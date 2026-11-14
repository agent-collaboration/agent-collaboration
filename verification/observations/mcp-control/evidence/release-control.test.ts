import assert from "node:assert/strict";
import { mkdtempSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createConnection } from "node:net";
import { once } from "node:events";
import { test } from "node:test";
import { listenForHeldRelease } from "../../../fixtures/held-release-control.ts";
import { releaseHeldWork } from "./release-control.ts";

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

test("malformed release JSON is refused and recorded without releasing work", async (): Promise<void> => {
  const root = mkdtempSync(join(tmpdir(), "release-malformed-"));
  const socketPath = join(root, "release");
  const replies: unknown[] = [];
  const server = await listenForHeldRelease({
    socketPath,
    releaseOwner(): boolean {
      assert.fail("malformed input reached owner");
    },
    recordReply(reply): void {
      replies.push(reply);
    },
  });
  try {
    const client = createConnection(socketPath);
    await once(client, "connect");
    const response = once(client, "data");
    client.end("not json\n");
    const [data] = await response;
    assert.deepEqual(JSON.parse(String(data)), { kind: "refused", ownerId: "invalid" });
    assert.deepEqual(replies, [{ kind: "refused", ownerId: "invalid" }]);
    client.destroy();
  } finally {
    server.close();
    await once(server, "close");
    rmSync(root, { recursive: true, force: true });
  }
});

test("an early controller disconnect leaves the release service alive", async (): Promise<void> => {
  const root = mkdtempSync(join(tmpdir(), "release-disconnect-"));
  const socketPath = join(root, "release");
  let observed: (() => void) | undefined;
  const released = new Promise<void>((resolve): void => {
    observed = resolve;
  });
  const server = await listenForHeldRelease({
    socketPath,
    releaseOwner(): boolean {
      return true;
    },
    recordReply(): void {
      observed?.();
    },
  });
  try {
    const client = createConnection(socketPath);
    await once(client, "connect");
    client.write('{"ownerId":"first"}\n', (): void => {
      client.destroy();
    });
    await released;
    await assert.doesNotReject(releaseHeldWork(socketPath, "second"));
  } finally {
    server.close();
    await once(server, "close");
    rmSync(root, { recursive: true, force: true });
  }
});
