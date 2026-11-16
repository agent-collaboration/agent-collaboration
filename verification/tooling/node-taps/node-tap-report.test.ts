import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { buildReport, readRecordings, reportPasses, tableFindings } from "./node-tap-report.ts";
import type { TapRecording } from "./node-tap-report.ts";
import { REQUIRED_CATALOG_ROWS } from "./required-catalog-rows.ts";

/** A schema side with nothing unsettled. */
function settledSchema(): TapRecording["schema"] {
  return {
    expected: [],
    unlisted: [],
    fixed: [],
    coverageGaps: [],
    declaredGaps: [],
    injectionsNotShown: [],
    apparatus: [],
    unanswered: [],
  };
}

function recording(
  node: TapRecording["node"],
  row: string,
  records: TapRecording["records"],
): TapRecording {
  return {
    caseId: `${node}/${row.replaceAll(/[^a-z0-9]+/giu, "_")}`,
    node,
    case: row,
    catalogRow: row,
    records,
    outcome: { outcome: "passed" },
    schema: settledSchema(),
  };
}

const n6Produces: TapRecording["records"] = [
  {
    direction: "consumed",
    layer: "typedOutcome",
    connection: "case",
    intent: "product",
    reply: false,
    family: "acpSessionUpdate",
    counterpart: { kind: "AcpAgent" },
    kind: "session/update:agent_message_chunk",
  },
  {
    direction: "produced",
    layer: "typedOutcome",
    connection: "case",
    intent: "product",
    reply: false,
    family: "sessionEvent",
    counterpart: { kind: "Node", node: "N5" },
    kind: "ItemStarted(AgentMessage)",
  },
];

function everyRow(): TapRecording[] {
  return REQUIRED_CATALOG_ROWS.map((row) => recording(row.node, row.row, []));
}

test("a full, settled, table-consistent recording set passes", () => {
  const report = buildReport(everyRow());
  assert.deepEqual(report.missingRows, []);
  assert.ok(reportPasses(report));
});

test("a required row with no recording fails the report", () => {
  const report = buildReport(everyRow().slice(1));
  assert.equal(report.missingRows.length, 1);
  assert.equal(reportPasses(report), false);
});

test("a case whose schema side is unsettled fails the report", () => {
  const recordings = everyRow();
  const first = recordings[0];
  assert.ok(first !== undefined);
  recordings[0] = {
    ...first,
    schema: { ...settledSchema(), coverageGaps: ["consumed x #0: no raw"] },
  };
  const report = buildReport(recordings);
  assert.deepEqual(
    report.unsettled.map((finding) => finding.detail),
    ["1 coverage gaps"],
  );
  assert.equal(reportPasses(report), false);
});

test("an empty recording set fails rather than passing vacuously", () => {
  assert.equal(reportPasses(buildReport([])), false);
});

test("a row outside the inventory and an unsettled case fail the report", () => {
  const extra = recording("N6", "invented row", []);
  const unsettled = {
    ...recording("N3", "N3 catalog: TurnStarted", []),
    outcome: { outcome: "defectChanged" as const },
  };
  const report = buildReport([...everyRow(), extra, unsettled]);
  assert.deepEqual(report.unknownRows, ["N6: invented row"]);
  assert.equal(report.unsettled.length, 1);
  assert.equal(reportPasses(report), false);
});

test("table: N6 producing session events to the hub matches", () => {
  assert.deepEqual(tableFindings(recording("N6", "text chunk: new item", n6Produces)), []);
});

test("table: a node producing a family it does not own is a finding", () => {
  const findings = tableFindings(
    recording("N3", "TurnStarted", [
      {
        direction: "produced",
        layer: "typedOutcome",
        connection: "case",
        intent: "product",
        reply: false,
        family: "acpToAgent",
        counterpart: { kind: "AcpAgent" },
        kind: "session/prompt",
      },
    ]),
  );
  assert.equal(findings.length, 1);
  assert.match(findings[0]?.detail ?? "", /N3 is not a producer/u);
});

test("table: a wrong counterpart is a finding", () => {
  const findings = tableFindings(
    recording("N6", "text chunk: new item", [
      {
        direction: "produced",
        layer: "typedOutcome",
        connection: "case",
        intent: "product",
        reply: false,
        family: "sessionEvent",
        counterpart: { kind: "CodexCli" },
        kind: "ItemStarted(AgentMessage)",
      },
    ]),
  );
  assert.equal(findings.length, 1);
});

test("table: N10 taking a Codex request from N8 on its behalf matches", () => {
  const findings = tableFindings(
    recording("N10", "request and reply", [
      {
        direction: "consumed",
        layer: "typedOutcome",
        connection: "case",
        intent: "product",
        reply: false,
        family: "codexRequestToCodex",
        counterpart: { kind: "Node", node: "N8" },
        kind: "turn/start",
      },
      {
        direction: "produced",
        layer: "typedOutcome",
        connection: "case",
        intent: "product",
        reply: false,
        family: "codexRequestToCodex",
        counterpart: { kind: "ManagedCodex" },
        kind: "turn/start",
      },
    ]),
  );
  assert.deepEqual(findings, []);
});

test("recordings are read and schema-checked from <dir>/<node>/<case>.json", () => {
  const directory = mkdtempSync(join(tmpdir(), "d0-taps-"));
  mkdirSync(join(directory, "N6"));
  writeFileSync(
    join(directory, "N6", "case.json"),
    JSON.stringify(recording("N6", "text chunk: new item", n6Produces)),
  );
  assert.equal(readRecordings(directory).length, 1);
  writeFileSync(join(directory, "N6", "bad.json"), JSON.stringify({ caseId: "x" }));
  assert.throws(() => readRecordings(directory));
});

test("table: N8 handing a Codex request to N10 matches, N3 doing it does not", () => {
  const handOff = (node: TapRecording["node"]): TapRecording =>
    recording(node, "request and reply", [
      {
        direction: "produced",
        layer: "typedOutcome",
        connection: "case",
        intent: "product",
        reply: false,
        family: "codexRequestToCodex",
        counterpart: { kind: "Node", node: "N10" },
        kind: "turn/start",
      },
    ]);
  assert.deepEqual(tableFindings(handOff("N8")), []);
  assert.equal(tableFindings(handOff("N3")).length, 1);
});

test("table: a reply travels on its request's edge under the request's row", () => {
  const steeringReply = (reply: boolean): TapRecording =>
    recording("N6", "N6 catalog: Typed extension messages: Steering, injected while running", [
      {
        direction: "consumed",
        layer: "typedOutcome",
        connection: "case",
        intent: "product",
        reply,
        family: "acpToAgent",
        counterpart: { kind: "AcpAgent" },
        kind: "outcome:injected",
      },
    ]);
  assert.deepEqual(tableFindings(steeringReply(true)), []);
  assert.equal(tableFindings(steeringReply(false)).length, 2);
});

test("table: the declared N4 bypass is reported as its owner's defect, not as a finding", () => {
  const bypass = recording("N11", "N11 section: N11 --> N4; table: session command N1, N11 -> N4", [
    {
      direction: "produced",
      layer: "typedOutcome",
      connection: "case",
      intent: "product",
      reply: false,
      family: "sessionCommand",
      counterpart: { kind: "ProviderSupervisor" },
      kind: "cancel",
    },
  ]);
  assert.deepEqual(tableFindings(bypass), []);
  const report = buildReport([bypass]);
  assert.equal(report.declaredDefectEdges.length, 1);
  assert.match(report.declaredDefectEdges[0]?.detail ?? "", /^external:cli-mcp-lane: /u);
  const elsewhere = recording("N1", "N1 node: session commands -> N4", [
    {
      direction: "produced",
      layer: "typedOutcome",
      connection: "case",
      intent: "product",
      reply: false,
      family: "sessionCommand",
      counterpart: { kind: "ProviderSupervisor" },
      kind: "cancel",
    },
  ]);
  assert.equal(tableFindings(elsewhere).length, 1);
});

test("table: the declared N4 bypass exempts only its declared orientation", () => {
  const reversed = recording(
    "N11",
    "N11 section: N11 --> N4; table: session command N1, N11 -> N4",
    [
      {
        direction: "consumed",
        layer: "typedOutcome",
        connection: "case",
        intent: "product",
        reply: false,
        family: "sessionCommand",
        counterpart: { kind: "ProviderSupervisor" },
        kind: "unexpected-supervisor-command",
      },
    ],
  );
  assert.ok(tableFindings(reversed).length > 0);
  assert.equal(buildReport([reversed]).declaredDefectEdges.length, 0);
  const asReply = recording(
    "N11",
    "N11 section: N11 --> N4; table: session command N1, N11 -> N4",
    [
      {
        direction: "produced",
        layer: "typedOutcome",
        connection: "case",
        intent: "product",
        reply: true,
        family: "sessionCommand",
        counterpart: { kind: "ProviderSupervisor" },
        kind: "cancel",
      },
    ],
  );
  assert.ok(tableFindings(asReply).length > 0);
});
