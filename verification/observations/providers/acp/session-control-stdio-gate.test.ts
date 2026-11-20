import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync, rmdirSync, unlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { createFixtureDirectory } from "../../../test-support/fixture-directory.ts";
import {
  SessionControlStdioGate,
  wireEntrySchema,
  type Hold,
  type WireEntry,
} from "./session-control-stdio-gate.ts";

const rpc = (frame: object): string => JSON.stringify({ jsonrpc: "2.0", ...frame });

test("Node preload holds only the selected real stdout frame and leaves the import probe intact", (context) => {
  const directory = createFixtureDirectory("session-control-gate");
  const hold = join(directory, "hold.json");
  const wire = join(directory, "wire.jsonl");
  const environment = { ...process.env, CODEX_ROUTER_SESSION_CONTROL_GATE: directory };
  const preload = new URL("session-control-stdio-gate.ts", import.meta.url).href;
  context.after(() => {
    for (const path of [hold, wire]) {
      try {
        unlinkSync(path);
      } catch (error) {
        if (!(error instanceof Error && "code" in error && error.code === "ENOENT")) throw error;
      }
    }
    rmdirSync(directory);
  });
  {
    const probe = spawnSync(process.execPath, ["--import", preload, "-e", "console.log('probe')"], {
      env: environment,
      encoding: "utf8",
      timeout: 5000,
    });
    assert.equal(probe.status, 0, probe.stderr);
    assert.equal(probe.stdout, "probe\n");
    assert.throws(() => readFileSync(wire), { code: "ENOENT" });
    writeFileSync(hold, JSON.stringify({ method: "session/close", sessionId: "A" }), {
      mode: 0o600,
    });
    const frames = [
      rpc({ id: 0, method: "initialize", params: { protocolVersion: 1 } }),
      rpc({ id: 1, method: "session/close", params: { sessionId: "A" } }),
      rpc({ id: 2, method: "session/prompt", params: { sessionId: "B" } }),
    ];
    const peer = spawnSync(
      process.execPath,
      [
        "--import",
        preload,
        "--input-type=module",
        "-e",
        "import {createInterface} from 'node:readline'; " +
          "for await (const line of createInterface({input:process.stdin})) {" +
          "const {id}=JSON.parse(line); console.log(JSON.stringify({jsonrpc:'2.0',id,result:{}}));}",
      ],
      { env: environment, encoding: "utf8", input: frames.join("\n") + "\n", timeout: 5000 },
    );
    assert.equal(peer.status, 0, peer.stderr);
    assert.equal(
      peer.stdout,
      rpc({ id: 0, result: {} }) + "\n" + rpc({ id: 2, result: {} }) + "\n",
    );
    const entries = readFileSync(wire, "utf8")
      .trimEnd()
      .split("\n")
      .map((line) => wireEntrySchema.parse(JSON.parse(line)));
    assert.deepEqual(
      entries.filter((entry) => entry.disposition === "withheld").map((entry) => entry.frame?.id),
      [1],
    );
  }
});

test("a restore hold withholds the exact reply and target replay while the other session completes", () => {
  const entries: WireEntry[] = [];
  let hold: Hold | null = { method: "session/load", sessionId: "A" };
  const gate = new SessionControlStdioGate(
    "connection",
    () => hold,
    (entry) => entries.push(entry),
  );
  const host = (frame: object) => gate.forward("hostToAgent", rpc(frame));
  const agent = (frame: object) => gate.forward("agentToHost", rpc(frame));
  assert.equal(host({ id: 0, method: "initialize", params: { protocolVersion: 1 } }), true);
  assert.equal(
    agent({ id: 0, result: { agentCapabilities: {}, _meta: { steering: { supported: false } } } }),
    true,
  );
  assert.equal(host({ id: 1, method: "session/load", params: { sessionId: "A" } }), true);
  assert.equal(host({ id: 2, method: "session/prompt", params: { sessionId: "B" } }), true);
  assert.equal(
    agent({ id: 1, method: "session/request_permission", params: { sessionId: "A" } }),
    true,
  );
  assert.equal(host({ id: 1, result: {} }), true);
  assert.equal(agent({ id: 2, result: { stopReason: "end_turn" } }), true);
  assert.equal(
    agent({
      method: "session/update",
      params: { sessionId: "A", update: { sessionUpdate: "user_message_chunk" } },
    }),
    false,
  );
  hold = null;
  assert.equal(agent({ id: 1, result: {} }), false);
  assert.equal(host({ id: 3, method: "session/load", params: { sessionId: "A" } }), true);
  assert.equal(agent({ id: 3, result: {} }), true);
  assert.deepEqual(
    entries
      .filter(({ disposition }) => disposition === "withheld")
      .map(({ method, sessionId }) => [method, sessionId]),
    [
      ["session/update", "A"],
      ["session/load", "A"],
    ],
  );
  assert.deepEqual(
    entries.map(({ sequence }) => sequence),
    entries.map((_, index) => index),
  );
});

test("the prompt hold is token-bound and sends the cancel notification without forwarding the terminal reply", () => {
  const entries: WireEntry[] = [];
  const gate = new SessionControlStdioGate(
    "cancel",
    () => ({
      method: "session/prompt",
      sessionId: "A",
      promptToken: "[selected]",
    }),
    (entry) => entries.push(entry),
  );
  const host = (frame: object) => gate.forward("hostToAgent", rpc(frame));
  const agent = (frame: object) => gate.forward("agentToHost", rpc(frame));
  host({
    method: "initialize",
    id: 0,
    params: { protocolVersion: 1, authorization: "discard this" },
  });
  host({
    method: "session/prompt",
    id: 1,
    params: { sessionId: "A", prompt: [{ text: "earlier" }] },
  });
  assert.equal(agent({ id: 1, result: {} }), true);
  host({
    method: "session/prompt",
    id: 2,
    params: { sessionId: "A", prompt: [{ text: "[selected]" }] },
  });
  assert.equal(host({ method: "session/cancel", params: { sessionId: "A" } }), true);
  assert.equal(agent({ id: 2, result: { stopReason: "cancelled" } }), false);
  assert.equal(JSON.stringify(entries).includes("discard this"), false);
  assert.equal(entries.find(({ method }) => method === "session/cancel")?.frame?.id, undefined);
  assert.deepEqual(
    entries.filter(({ disposition }) => disposition === "withheld").map(({ frame }) => frame?.id),
    [2],
  );
});
