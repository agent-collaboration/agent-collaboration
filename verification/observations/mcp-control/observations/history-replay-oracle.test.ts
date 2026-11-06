// Replayed history in the producer's shape. The faithful baseline is structurally the first
// passing controlled run's post-load baseline (mcf13, events_observe after conversation_load):
// settings and capabilities, then each earlier exchange as a historical turn whose terminal is
// `stop_reason: {unknown: "replayed"}` (acp-client-runtime provider_session_restore.rs:199-216),
// then idle. IDs are substituted.
import assert from "node:assert/strict";
import { test } from "node:test";
import { decideNoReplay, decideReplayedHistory } from "./history-replay-oracle.ts";
import type { NumberedEvent } from "./observation-ledger.ts";
import { sessionEventSchema } from "./observation-models.ts";

type Raw = Readonly<Record<string, unknown>>;
const REPLAYED = { unknown: "replayed" };
const RECORDED = [
  { prompt: "facade-controlled-proof", reply: "beforeafter" },
  { prompt: "facade-controlled-proof", reply: "beforeafter" },
];

function historicalTurn(
  index: number,
  exchange: { prompt: string; reply: string },
  stop: unknown,
): Raw[] {
  const turnId = `history-turn-${index}`;
  return [
    { kind: "turnStarted", turnId, inputId: `history-input-${index}` },
    {
      kind: "itemStarted",
      item: { itemId: `u-${index}`, kind: { kind: "userMessage" }, text: exchange.prompt },
    },
    { kind: "itemCompleted", itemId: `u-${index}` },
    {
      kind: "itemUpdated",
      item: { itemId: `a-${index}`, kind: { kind: "agentMessage" }, text: exchange.reply },
    },
    { kind: "itemCompleted", itemId: `a-${index}` },
    { kind: "turnEnded", turnId, outcome: { kind: "ended", stop_reason: stop, local_cause: null } },
  ];
}

function baseline(turns: readonly Raw[][]): NumberedEvent[] {
  const events: Raw[] = [
    { kind: "settingsChanged", settings: {} },
    { kind: "capabilitiesChanged", capabilities: {} },
    ...turns.flat(),
    { kind: "stateChanged", state: { kind: "idle" } },
  ];
  return events.map((event, index) => ({
    sequence: BigInt(index + 1),
    // Parsed through the wire schema, so a fixture that is not a real event fails here.
    event: sessionEventSchema.parse(event),
  }));
}

const faithful = (): Raw[][] =>
  RECORDED.map((exchange, index) => historicalTurn(index, exchange, REPLAYED));

test("the recorded conversation replayed in order as historical turns is complete", () => {
  assert.deepEqual(decideReplayedHistory(baseline(faithful()), RECORDED), { kind: "complete" });
});

test("missing, extra, reordered, canned or foreign history is refused", () => {
  const first = { prompt: "first prompt", reply: "first reply" };
  const distinct = [first, { prompt: "second prompt", reply: "second reply" }];
  const turns = distinct.map((exchange, index) => historicalTurn(index, exchange, REPLAYED));
  const cases: [string, NumberedEvent[], readonly { prompt: string; reply: string }[]][] = [
    ["missing", baseline(turns.slice(0, 1)), distinct],
    ["extra", baseline([...turns, historicalTurn(2, first, REPLAYED)]), distinct],
    ["reordered", baseline([...turns].reverse()), distinct],
    [
      "canned",
      baseline(
        distinct.map((_, index) =>
          historicalTurn(index, { prompt: "canned", reply: "canned" }, REPLAYED),
        ),
      ),
      distinct,
    ],
    ["empty", baseline([]), distinct],
  ];
  for (const [name, events, recorded] of cases)
    assert.equal(decideReplayedHistory(events, recorded).kind, "failed", name);
});

test("a live-looking terminal or an injected input inside replayed history is refused", () => {
  const live = RECORDED.map((exchange, index) => historicalTurn(index, exchange, "endTurn"));
  assert.equal(decideReplayedHistory(baseline(live), RECORDED).kind, "failed");
  const [steered, ...rest] = faithful();
  if (steered === undefined) throw new Error("the faithful fixture holds no turn");
  steered.splice(1, 0, { kind: "inputAccepted", inputId: "steer", turnId: "history-turn-0" });
  assert.equal(decideReplayedHistory(baseline([steered, ...rest]), RECORDED).kind, "failed");
});

test("after resume, no historical turn may appear", () => {
  assert.deepEqual(decideNoReplay(baseline([])), { kind: "complete" });
  assert.equal(decideNoReplay(baseline(faithful())).kind, "failed");
});
