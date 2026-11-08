import assert from "node:assert/strict";
import { once } from "node:events";
import { mkdtemp, rm } from "node:fs/promises";
import { createConnection } from "node:net";
import { test } from "node:test";
import { ReceiverCustodyBarrier } from "./receiver-custody-barrier.ts";

test(
  "the real socket marker parks each receipt until release and later receipts pass through",
  { timeout: 5000 },
  async (): Promise<void> => {
    const root = await mkdtemp("/private/tmp/d4b-");
    const barrier = await ReceiverCustodyBarrier.listen(`${root}/accepted.sock`);
    const first = createConnection(barrier.path),
      second = createConnection(barrier.path);
    const replies: Buffer[] = [];
    first.on("data", (bytes: Buffer): void => {
      replies.push(Buffer.from(bytes));
    });
    try {
      await Promise.all([once(first, "connect"), once(second, "connect")]);
      first.write("inbox-");
      first.write("accepted\n");
      second.write("inbox-accepted\n");
      assert.equal(await barrier.waitForReached(2, 1000), 2);
      assert.deepEqual(barrier.snapshot(), {
        reached: 2,
        held: 2,
        phase: "holding",
        runtimeQualified: false,
      });
      assert.deepEqual(replies, []);
      const firstEnded = once(first, "end"),
        secondEnded = once(second, "end");
      second.resume();
      assert.equal(barrier.release(), 2);
      assert.equal(barrier.release(), 0);
      await Promise.all([firstEnded, secondEnded]);
      assert.deepEqual(Buffer.concat(replies), Buffer.from([1]));
      const later = createConnection(barrier.path);
      try {
        await once(later, "connect");
        const answer = once(later, "data");
        later.write("inbox-accepted\n");
        const [bytes] = await answer;
        assert.deepEqual(bytes, Buffer.from([1]));
        assert.equal(await barrier.waitForReached(3, 1000), 3);
        assert.equal(barrier.snapshot().held, 0);
      } finally {
        later.destroy();
      }
      barrier.requireHealthy();
    } finally {
      first.destroy();
      second.destroy();
      await barrier.close();
      await rm(root, { recursive: true });
    }
  },
);

test(
  "wrong or repeated marker bytes latch a failure rather than reaching the custody cut",
  { timeout: 5000 },
  async (): Promise<void> => {
    for (const marker of ["unrelated\n", "inbox-accepted\nextra"]) {
      const root = await mkdtemp("/private/tmp/d4b-");
      const barrier = await ReceiverCustodyBarrier.listen(`${root}/accepted.sock`);
      const client = createConnection(barrier.path);
      client.on("error", (): void => {});
      try {
        await once(client, "connect");
        const rejected = assert.rejects(barrier.waitForReached(1, 1000), /barrier invalid/);
        client.write(marker);
        await rejected;
        assert.throws((): number => barrier.release(), /barrier invalid/);
        assert.throws((): void => barrier.requireHealthy(), /barrier invalid/);
      } finally {
        client.destroy();
        await barrier.close();
        await rm(root, { recursive: true });
      }
    }
  },
);

test(
  "a missing or partial marker is never a successful reach and closing settles its waiter",
  { timeout: 5000 },
  async (): Promise<void> => {
    const root = await mkdtemp("/private/tmp/d4b-");
    const barrier = await ReceiverCustodyBarrier.listen(`${root}/accepted.sock`);
    const client = createConnection(barrier.path);
    try {
      await once(client, "connect");
      client.write("inbox-");
      await assert.rejects(barrier.waitForReached(1, 25), /reach deadline/);
      const rejected = assert.rejects(barrier.waitForReached(1, 1000), /closed before reach/);
      await barrier.close();
      await rejected;
      assert.equal(barrier.snapshot().reached, 0);
      assert.throws((): number => barrier.release(), /already closed/);
    } finally {
      client.destroy();
      await barrier.close();
      await rm(root, { recursive: true });
    }
  },
);
