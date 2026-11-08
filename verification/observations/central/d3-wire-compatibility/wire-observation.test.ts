import { test } from "node:test";
import assert from "node:assert/strict";
import {
  CompatibilityWireObservation,
  requireCompatibleWire,
  requireExactBacklog,
} from "./wire-observation.ts";
import { SessionAudit } from "../../../access/session-audit.ts";

test("legacy Welcome and retained notice page are observed without optional capability leakage", (): void => {
  const observation = new CompatibilityWireObservation();
  observation.observe({ kind: "welcome", value: { sessionId: "fixture", databases: [] } });
  observation.observe({
    kind: "answer",
    value: {
      body: {
        kind: "consumer",
        value: {
          kind: "read",
          value: {
            kind: "notices",
            value: {
              kind: "page",
              value: {
                notices: [{ position: 99n }, { position: 100n }],
                asOf: "2026-11-08T00:00:00Z",
              },
            },
          },
        },
      },
    },
  });
  requireCompatibleWire(observation.snapshot());
  assert.deepEqual(observation.snapshot().noticePositions, ["99", "100"]);
});

test("a negotiated field sent to the old client and a refusal cannot receive compatibility credit", (): void => {
  for (const wrong of [
    { kind: "welcome", value: { capabilities: { batches: true } } },
    { kind: "requestBatch", value: { items: [] } },
    { kind: "answer", value: { body: { kind: "refused", value: "invalidShape" } } },
  ]) {
    const observation = new CompatibilityWireObservation();
    observation.observe({ kind: "welcome", value: {} });
    observation.observe(wrong);
    assert.throws((): void => requireCompatibleWire(observation.snapshot()), /legacy wire/);
  }
});

test("the backlog evaluator rejects a missing item or a duplicate push and accepts unrelated traffic", (): void => {
  const expected = Array.from({ length: 50 }, (_, index): string => `item-${index}`);
  requireExactBacklog(expected, ["unrelated", ...expected]);
  assert.throws((): void => requireExactBacklog(expected, expected.slice(1)), /50 exact/);
  assert.throws(
    (): void => requireExactBacklog(expected, [...expected, expected[0] ?? ""]),
    /50 exact/,
  );
});

test("the real binary-frame audit invokes the callback only after complete fragmented payload arrival", (): void => {
  const seen: unknown[] = [];
  const audit = new SessionAudit((value): void => {
    seen.push(value);
  });
  const connection = audit.opened();
  audit.receive(connection, Buffer.from("HTTP/1.1 101 Switching Protocols\r\n\r\n"));
  const value = { kind: "welcome", value: { sessionId: "fixture", databases: [] } };
  const bytes = Buffer.from(JSON.stringify(value));
  const cut = 20;
  audit.receive(connection, Buffer.concat([Buffer.from([2, cut]), bytes.subarray(0, cut)]));
  assert.deepEqual(seen, []);
  audit.receive(
    connection,
    Buffer.concat([Buffer.from([128, bytes.length - cut]), bytes.subarray(cut)]),
  );
  assert.deepEqual(seen, [value]);
  assert.equal(audit.snapshot().invalidConnections, 0);
});
