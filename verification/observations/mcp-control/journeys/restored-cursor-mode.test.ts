import assert from "node:assert/strict";
import { test } from "node:test";
import type { WireEntry } from "../../providers/acp/session-control-stdio-gate.ts";
import { confirmedCursorAutoAfterLoad } from "./restored-cursor-mode.ts";

const sessionId = "restored-cursor";
const loadEntries = [
  {
    connection: "cursor",
    sessionId,
    sequence: 6,
    monotonicMs: 0,
    disposition: "forwarded",
    direction: "hostToAgent",
    method: "session/load",
    frame: { jsonrpc: "2.0", method: "session/load", id: 4, params: { sessionId } },
  },
  {
    connection: "cursor",
    sessionId,
    sequence: 10,
    monotonicMs: 2,
    disposition: "forwarded",
    direction: "agentToHost",
    method: "session/load",
    frame: {
      jsonrpc: "2.0",
      id: 4,
      result: {
        modes: { currentModeId: "agent" },
        configOptions: [{ category: "mode", currentValue: "agent" }],
      },
    },
  },
] as const satisfies readonly WireEntry[];

test("the forwarded exact-session load can confirm Cursor Auto", () => {
  assert.deepEqual(confirmedCursorAutoAfterLoad(loadEntries, sessionId), {
    load: 6,
    reply: 10,
    modeFrame: 10,
  });
});

test("an absent, mismatched or projected load cannot confirm reopened Cursor Auto", () => {
  const [load, reply] = loadEntries;
  const mismatched = { ...reply, frame: { ...reply.frame, id: 5 } };
  const missingMode = { ...reply, frame: { ...reply.frame, result: {} } };
  const wrongMode = {
    ...reply,
    frame: { ...reply.frame, result: { currentModeId: "plan" } },
  };
  const projected = { ...wrongMode, forwardedFrame: reply.frame };

  for (const candidate of [mismatched, missingMode, wrongMode, projected])
    assert.equal(confirmedCursorAutoAfterLoad([load, candidate], sessionId), undefined);
});

test("a changed mode wins over the earlier load report", () => {
  const update = {
    ...loadEntries[1],
    sequence: 11,
    method: "session/update",
    frame: {
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId,
        update: { sessionUpdate: "current_mode_update", currentModeId: "plan" },
      },
    },
  } satisfies WireEntry;

  assert.equal(confirmedCursorAutoAfterLoad([...loadEntries, update], sessionId), undefined);
});
