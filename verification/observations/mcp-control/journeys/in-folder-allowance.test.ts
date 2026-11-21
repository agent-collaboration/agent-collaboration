import assert from "node:assert/strict";
import { mkdtempDisposableSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import type { WireEntry } from "../../providers/acp/session-control-stdio-gate.ts";
import { confirmsRefusedEdit, markerEvidence } from "./in-folder-allowance.ts";

const wireContext = {
  connection: "provider",
  sessionId: "observed-session",
  disposition: "forwarded",
} as const;

const cancelledExchange = {
  reply: {
    ...wireContext,
    sequence: 159,
    monotonicMs: 1,
    direction: "hostToAgent",
    method: "session/request_permission",
    frame: { jsonrpc: "2.0", id: 1, result: { outcome: { outcome: "cancelled" } } },
  },
  cancel: {
    ...wireContext,
    sequence: 160,
    monotonicMs: 2,
    direction: "hostToAgent",
    method: "session/cancel",
    frame: {
      jsonrpc: "2.0",
      method: "session/cancel",
      params: { sessionId: wireContext.sessionId },
    },
  },
  terminal: {
    ...wireContext,
    sequence: 161,
    monotonicMs: 3,
    direction: "agentToHost",
    method: "session/prompt",
    frame: { jsonrpc: "2.0", id: 8, result: { stopReason: "cancelled" } },
  },
} satisfies Record<"reply" | "cancel" | "terminal", WireEntry>;

test("a refused delete accepts the exact seed and a cancelled provider reply", () => {
  using directory = mkdtempDisposableSync(join(tmpdir(), "allowance-refusal-"));
  const path = join(directory.path, "marker.txt");
  writeFileSync(path, "OK\n");

  const marker = markerEvidence(path);

  assert.equal(
    confirmsRefusedEdit({ action: "delete", marker, exchange: cancelledExchange }),
    true,
  );
});

test("a refused delete rejects marker bytes that trim to the seed text", () => {
  using directory = mkdtempDisposableSync(join(tmpdir(), "allowance-refusal-"));
  const path = join(directory.path, "marker.txt");
  writeFileSync(path, "OK");

  const marker = markerEvidence(path);

  assert.equal(
    confirmsRefusedEdit({ action: "delete", marker, exchange: cancelledExchange }),
    false,
    "the missing newline changes the seed",
  );
});

test("a refused delete requires the provider's terminal stop reason to be cancelled", () => {
  using directory = mkdtempDisposableSync(join(tmpdir(), "allowance-refusal-"));
  const path = join(directory.path, "marker.txt");
  writeFileSync(path, "OK\n");
  const exchange = {
    ...cancelledExchange,
    terminal: {
      ...cancelledExchange.terminal,
      frame: { ...cancelledExchange.terminal.frame, result: { stopReason: "end_turn" } },
    },
  };

  const marker = markerEvidence(path);

  assert.equal(confirmsRefusedEdit({ action: "delete", marker, exchange }), false);
});
