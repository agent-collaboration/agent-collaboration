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

interface BehaviorPeer {
  readonly root: string;
  readonly receipt: string;
  readonly sentinel: string;
  readonly frames: Record<string, unknown>[];
  readonly initialized: Record<string, unknown>;
  send(id: string | undefined, method: string, params: unknown): void;
  until(predicate: (frame: Record<string, unknown>) => boolean): Promise<Record<string, unknown>>;
  request(id: string, method: string, params: unknown): Promise<Record<string, unknown>>;
  finish(): Promise<void>;
}

async function openBehaviorPeer(props: {
  readonly context: TestContext;
  readonly profile: "lifecycle" | "manual";
  readonly release: boolean;
  readonly authenticate?: boolean;
}): Promise<BehaviorPeer> {
  const root = mkdtempSync(join(tmpdir(), "facade-lifecycle-"));
  const receipt = join(root, "receipt.jsonl");
  const sentinel = join(root, "release");
  const authentication = props.authenticate !== false;
  const child = spawn(process.execPath, [
    new URL("./behavior-agent.ts", import.meta.url).pathname,
    receipt,
    props.profile,
    ...(authentication ? ["cursor_login"] : []),
    ...(props.release ? [sentinel] : []),
  ]);
  const settled = once(child, "close");
  let stderr = "";
  child.stderr.setEncoding("utf8").on("data", (chunk: string): void => {
    stderr += chunk;
  });
  const lines = createInterface({ input: child.stdout });
  props.context.after(async (): Promise<void> => {
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
      if (next.done) throw new Error(`fixture exited before expected frame: ${stderr}`);
      const frame = objectSchema.parse(decode(next.value));
      frames.push(frame);
      if (predicate(frame)) return frame;
    }
  }
  function send(id: string | undefined, method: string, params: unknown): void {
    child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
  }
  async function request(
    id: string,
    method: string,
    params: unknown,
  ): Promise<Record<string, unknown>> {
    send(id, method, params);
    return until((frame) => frame.id === id);
  }
  async function finish(): Promise<void> {
    child.stdin.end();
    for (;;) {
      const next = await iterator.next();
      if (next.done) break;
      frames.push(objectSchema.parse(decode(next.value)));
    }
    const [code] = await settled;
    assert.equal(code, 0, stderr);
    assert.equal(stderr, "");
  }
  const initialized = await request("initialize", "initialize", {});
  if (authentication) await request("authenticate", "authenticate", { methodId: "cursor_login" });
  return { root, receipt, sentinel, frames, initialized, send, until, request, finish };
}

async function createKnownSession(peer: BehaviorPeer, requestId: string): Promise<string> {
  const reply = await peer.request(requestId, "session/new", { cwd: peer.root, mcpServers: [] });
  const sessionId = field(reply, "result", "sessionId");
  if (typeof sessionId !== "string") throw new Error("created Session ID missing");
  return sessionId;
}
function messageChunks(frames: readonly Record<string, unknown>[], sessionId: string): unknown[] {
  return frames
    .filter(
      (frame) =>
        frame.method === "session/update" &&
        field(frame, "params", "sessionId") === sessionId &&
        ["user_message_chunk", "agent_message_chunk"].includes(
          String(field(frame, "params", "update", "sessionUpdate")),
        ),
    )
    .map((frame) => [
      field(frame, "params", "update", "sessionUpdate"),
      field(frame, "params", "update", "content", "text"),
    ]);
}
async function completePrompt(
  peer: BehaviorPeer,
  sessionId: string,
  requestId: string,
  text: string,
): Promise<void> {
  const start = peer.frames.length;
  const reply = await peer.request(requestId, "session/prompt", {
    sessionId,
    prompt: [{ type: "text", text }],
  });
  assert.equal(field(reply, "result", "stopReason"), "end_turn");
  assert.deepEqual(messageChunks(peer.frames.slice(start), sessionId), [
    ["agent_message_chunk", "before"],
    ["agent_message_chunk", "after"],
  ]);
}
function isAgentChunk(frame: Record<string, unknown>, text: string): boolean {
  return (
    frame.method === "session/update" &&
    field(frame, "params", "update", "sessionUpdate") === "agent_message_chunk" &&
    field(frame, "params", "update", "content", "text") === text
  );
}
function assertInvalidReply(reply: Record<string, unknown>): void {
  assert.ok(
    Object.hasOwn(reply, "error"),
    "invalid Session input must be refused, not acknowledged",
  );
  assert.equal(field(reply, "error", "code"), -32602n);
}

test(
  "lifecycle capabilities depend on the named profile, not authentication or release",
  { timeout: 10000 },
  async (context: TestContext): Promise<void> => {
    // Arrange/Act: real initialize replies under both profiles and both release configurations.
    for (const profile of ["manual", "lifecycle"] as const) {
      for (const release of [false, true]) {
        const peer = await openBehaviorPeer({ context, profile, release });
        const capabilities = field(peer.initialized, "result", "agentCapabilities");
        // Assert: legacy advertisements remain unchanged; lifecycle support is explicit.
        assert.equal(field(capabilities, "loadSession"), true);
        assert.deepEqual(
          field(capabilities, "sessionCapabilities"),
          profile === "lifecycle" ? { resume: {}, close: {} } : undefined,
        );
        await peer.finish();
      }
    }
    const withoutAuthentication = await openBehaviorPeer({
      context,
      profile: "lifecycle",
      release: false,
      authenticate: false,
    });
    assert.deepEqual(
      field(
        withoutAuthentication.initialized,
        "result",
        "agentCapabilities",
        "sessionCapabilities",
      ),
      { resume: {}, close: {} },
    );
    await withoutAuthentication.finish();
  },
);

test(
  "lifecycle close, resume and load retain the same known Session and its own observed history",
  { timeout: 5000 },
  async (context: TestContext): Promise<void> => {
    // Arrange: two distinct real Sessions with different observed input histories.
    const peer = await openBehaviorPeer({ context, profile: "lifecycle", release: false });
    const session = await createKnownSession(peer, "new-a");
    const foreign = await createKnownSession(peer, "new-b");
    assert.notEqual(session, foreign);
    await completePrompt(peer, session, "first", "first owned input");
    await completePrompt(peer, foreign, "foreign", "foreign input");
    // Act/Assert: resume emits no replay; load replays only actual A history before its reply.
    await peer.request("close-1", "session/close", { sessionId: session });
    const resumeStart = peer.frames.length;
    assert.deepEqual(
      field(
        await peer.request("resume", "session/resume", {
          sessionId: session,
          cwd: peer.root,
          mcpServers: [],
        }),
        "result",
      ),
      {},
    );
    assert.deepEqual(messageChunks(peer.frames.slice(resumeStart), session), []);
    await completePrompt(peer, session, "second", "second owned input");
    await peer.request("close-2", "session/close", { sessionId: session });
    const loadStart = peer.frames.length;
    assert.deepEqual(
      field(
        await peer.request("load", "session/load", {
          sessionId: session,
          cwd: peer.root,
          mcpServers: [],
        }),
        "result",
      ),
      {},
    );
    assert.deepEqual(messageChunks(peer.frames.slice(loadStart), session), [
      ["user_message_chunk", "first owned input"],
      ["agent_message_chunk", "before"],
      ["agent_message_chunk", "after"],
      ["user_message_chunk", "second owned input"],
      ["agent_message_chunk", "before"],
      ["agent_message_chunk", "after"],
    ]);
    await completePrompt(peer, session, "third", "after real load");
    await peer.finish();
  },
);

test(
  "lifecycle rejects unknown or closed Sessions and foreign working directories",
  { timeout: 5000 },
  async (context: TestContext): Promise<void> => {
    // Arrange: one known Session; only session/new may introduce an identity.
    const peer = await openBehaviorPeer({ context, profile: "lifecycle", release: false });
    const session = await createKnownSession(peer, "new");
    // Act/Assert: bad restore/prompt inputs are refusals, never generic success ACKs.
    for (const method of ["session/resume", "session/load"]) {
      for (const [sessionId, cwd] of [
        ["unknown", peer.root],
        [session, `${peer.root}/foreign`],
        [session, "relative"],
      ]) {
        assertInvalidReply(
          await peer.request(`${method}-${sessionId}-${cwd}`, method, {
            sessionId,
            cwd,
            mcpServers: [],
          }),
        );
      }
    }
    assertInvalidReply(
      await peer.request("unknown-prompt", "session/prompt", { sessionId: "unknown", prompt: [] }),
    );
    assertInvalidReply(
      await peer.request("unknown-close", "session/close", { sessionId: "unknown" }),
    );
    await peer.request("close", "session/close", { sessionId: session });
    assertInvalidReply(
      await peer.request("closed-prompt", "session/prompt", { sessionId: session, prompt: [] }),
    );
    await peer.finish();
  },
);

for (const release of [false, true]) {
  test(
    `lifecycle held owner survives foreign control and closes once (release=${release})`,
    { timeout: 5000 },
    async (context: TestContext): Promise<void> => {
      // Arrange: A owns the held prompt; B remains independently usable.
      const peer = await openBehaviorPeer({ context, profile: "lifecycle", release });
      const owner = await createKnownSession(peer, "new-a");
      const foreign = await createKnownSession(peer, "new-b");
      peer.send("held", "session/prompt", {
        sessionId: owner,
        prompt: [{ type: "text", text: "hold" }],
      });
      await peer.until(
        (frame) => isAgentChunk(frame, "HOLDING") && field(frame, "params", "sessionId") === owner,
      );
      // Act: B's prompt completion is a causal barrier after its foreign cancellation.
      peer.send(undefined, "session/cancel", { sessionId: foreign });
      await completePrompt(peer, foreign, "barrier", "B barrier");
      await peer.request("foreign-close", "session/close", { sessionId: foreign });
      assert.equal(
        peer.frames.filter((frame) => frame.id === "held").length,
        0,
        "foreign control must not settle A",
      );
      await peer.request("owner-close", "session/close", { sessionId: owner });
      const terminals = peer.frames.filter((frame) => frame.id === "held");
      assert.equal(terminals.length, 1);
      assert.equal(field(terminals[0], "result", "stopReason"), "cancelled");
      peer.send(undefined, "session/cancel", { sessionId: owner });
      await peer.request("late-close", "session/close", { sessionId: owner });
      if (release) writeFileSync(peer.sentinel, "late release");
      await peer.finish();
      // Assert: no second settlement or invented wire cancellation/release after close.
      assert.equal(peer.frames.filter((frame) => frame.id === "held").length, 1);
      const rows = readFileSync(peer.receipt, "utf8")
        .trim()
        .split("\n")
        .map((line) => objectSchema.parse(decode(line)));
      assert.equal(rows.filter((row) => row.method === "session/cancel").length, 2);
      assert.equal(rows.filter((row) => row.kind === "release").length, 0);
    },
  );
}

test(
  "lifecycle refuses a second held admission without losing the first owner",
  { timeout: 5000 },
  async (context: TestContext): Promise<void> => {
    // Arrange: the existing sentinel is explicitly a single owned held-work gate.
    const peer = await openBehaviorPeer({ context, profile: "lifecycle", release: true });
    const owner = await createKnownSession(peer, "new-a");
    const other = await createKnownSession(peer, "new-b");
    peer.send("first-held", "session/prompt", {
      sessionId: owner,
      prompt: [{ type: "text", text: "hold" }],
    });
    await peer.until((frame) => isAgentChunk(frame, "HOLDING"));
    // Act: an invalid second hold must reply with refusal before any B output, not overwrite A.
    peer.send("second-held", "session/prompt", {
      sessionId: other,
      prompt: [{ type: "text", text: "hold" }],
    });
    const second = await peer.until(
      (frame) =>
        frame.id === "second-held" ||
        (isAgentChunk(frame, "HOLDING") && field(frame, "params", "sessionId") === other),
    );
    assert.equal(second.id, "second-held");
    assertInvalidReply(second);
    assert.equal(peer.frames.filter((frame) => frame.id === "first-held").length, 0);
    writeFileSync(peer.sentinel, "release first owner");
    assert.equal(
      field(await peer.until((frame) => frame.id === "first-held"), "result", "stopReason"),
      "end_turn",
    );
    await peer.finish();
    assert.equal(peer.frames.filter((frame) => frame.id === "first-held").length, 1);
    const rows = readFileSync(peer.receipt, "utf8")
      .trim()
      .split("\n")
      .map((line) => objectSchema.parse(decode(line)));
    assert.equal(rows.filter((row) => row.kind === "release" && row.sessionId === owner).length, 1);
  },
);
