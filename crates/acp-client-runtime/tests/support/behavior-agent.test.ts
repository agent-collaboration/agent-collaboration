import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { test, type TestContext } from "node:test";
import { decode, field, objectSchema } from "../../../../verification/exact-json.ts";

interface FixtureOutcome {
  readonly receipt: string;
  readonly frames: readonly Record<string, unknown>[];
  readonly terminal: Record<string, unknown>;
  readonly ending: "release" | "cancel" | "close";
}

function assertFixtureOutcome({ receipt, frames, terminal, ending }: FixtureOutcome): void {
  assert.equal(frames.filter((frame) => frame.id === "prompt").length, 1);
  const rows = readFileSync(receipt, "utf8")
    .trim()
    .split("\n")
    .map((line) => objectSchema.parse(decode(line)));
  assert.equal(rows.filter((row) => row.method === "session/prompt").length, 1);
  assert.equal(
    rows.filter((row) => row.method === "session/cancel").length,
    ending === "cancel" ? 1 : 0,
  );
  const releaseRows = rows.filter((row) => row.kind === "release");
  assert.equal(releaseRows.length, field(terminal, "result", "stopReason") === "end_turn" ? 1 : 0);
  if (ending === "release") {
    assert.equal(releaseRows[0]?.sessionId, "session-1");
    assert.deepEqual(
      frames
        .filter(
          (frame) =>
            frame.method === "session/update" &&
            field(frame, "params", "update", "sessionUpdate") === "agent_message_chunk",
        )
        .map((frame) => field(frame, "params", "update", "content", "text")),
      ["HOLDING", "after"],
    );
  }
}

for (const ending of ["release", "cancel", "close", "preexisting"] as const) {
  test(
    `held behavior peer ${ending} has one terminal owner and truthful receipts`,
    { timeout: 5000 },
    async (context: TestContext): Promise<void> => {
      // Arrange: real fixture process, with an owned absent sentinel and captured protocol/stderr.
      const root = mkdtempSync(join(tmpdir(), "facade-behavior-"));
      const receipt = join(root, "receipt.jsonl");
      const sentinel = join(root, "release");
      if (ending === "preexisting") writeFileSync(sentinel, "too early");
      const child = spawn(process.execPath, [
        new URL("./behavior-agent.ts", import.meta.url).pathname,
        receipt,
        "manual",
        "cursor_login",
        sentinel,
      ]);
      const settled = once(child, "close");
      let stderr = "";
      child.stderr.setEncoding("utf8").on("data", (chunk: string): void => {
        stderr += chunk;
      });
      const lines = createInterface({ input: child.stdout });
      context.after(async (): Promise<void> => {
        if (child.exitCode === null && child.signalCode === null) {
          child.kill("SIGTERM");
          await settled;
        }
        lines.close();
        rmSync(root, { recursive: true, force: true });
      });
      const iterator = lines[Symbol.asyncIterator]();
      const frames: Record<string, unknown>[] = [];
      async function until(
        predicate: (frame: Record<string, unknown>) => boolean,
      ): Promise<Record<string, unknown>> {
        for (;;) {
          const next = await iterator.next();
          assert.equal(next.done, false, `peer exited: ${stderr}`);
          if (next.done) throw new Error("peer exited before expected frame");
          const frame = objectSchema.parse(decode(next.value));
          frames.push(frame);
          if (predicate(frame)) return frame;
        }
      }
      function send(id: string | undefined, method: string, params: unknown): void {
        child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
      }
      send("initialize", "initialize", {});
      await until((frame) => frame.id === "initialize");
      send("authenticate", "authenticate", { methodId: "cursor_login" });
      await until((frame) => frame.id === "authenticate");
      send("create", "session/new", {});
      await until((frame) => frame.id === "create");
      send("prompt", "session/prompt", {
        sessionId: "session-1",
        prompt: [{ type: "text", text: "hold" }],
      });
      if (ending === "preexisting") {
        // A pre-existing sentinel must fail loudly, never provide a false detached-completion pass.
        const [code] = await settled;
        assert.notEqual(code, 0);
        assert.match(stderr, /release sentinel already exists before held prompt/);
        return;
      }
      await until(
        (frame) =>
          frame.method === "session/update" &&
          field(frame, "params", "update", "sessionUpdate") === "agent_message_chunk" &&
          field(frame, "params", "update", "content", "text") === "HOLDING",
      );

      // Act: release competes with an actual cancel or close on the same held prompt.
      if (ending === "cancel") send(undefined, "session/cancel", { sessionId: "session-1" });
      if (ending === "close") send("close", "session/close", { sessionId: "session-1" });
      writeFileSync(sentinel, "release");
      const terminal = await until((frame) => frame.id === "prompt");
      assert.ok(
        ["end_turn", "cancelled"].includes(String(field(terminal, "result", "stopReason"))),
      );
      if (ending === "release") assert.equal(field(terminal, "result", "stopReason"), "end_turn");
      child.stdin.end();
      for (;;) {
        const next = await iterator.next();
        if (next.done) break;
        frames.push(objectSchema.parse(decode(next.value)));
      }
      const [code] = await settled;

      // Assert: duplicate terminal replies, fabricated cancels and stale watcher work all fail.
      assert.equal(code, 0, stderr);
      assert.equal(stderr, "");
      assertFixtureOutcome({ receipt, frames, terminal, ending });
    },
  );
}
