/**
 * `node verification/tooling/node-taps/node-tap-report.ts <recording dir> [--json <out.json>]`
 *
 * The D0 producer/consumer report (R21, V-U10). It reads the node tap recordings the Rust tap
 * tests write when `CODEX_ROUTER_NODE_TAP_DIR` is set, and checks three things:
 * 1. coverage: every required catalog row (REQUIRED_CATALOG_ROWS, written from the Program Design,
 *    not from the recordings) has at least one recorded case; a missing row fails;
 * 2. settlement: every case passed or failed exactly as its listed defect;
 * 3. producer/consumer: every message a node recorded matches the Program Design table per
 *    message: the node is a declared producer (or intermediary) of what it produced and a declared
 *    consumer (or intermediary) of what it consumed, and the counterpart sits on the other side.
 * It exits 0 only when all three hold.
 */
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";

import { PRODUCER_CONSUMER_TABLE } from "./producer-consumer-table.ts";
import type { MessageFamily, Party } from "./producer-consumer-table.ts";
import {
  DECLARED_GAPS,
  DECLARED_STRUCTURAL_DEFECTS,
  REQUIRED_CATALOG_ROWS,
} from "./required-catalog-rows.ts";

const partySchema = z.enum([
  "N1",
  "N3",
  "N4",
  "N5",
  "N6",
  "N7",
  "N8",
  "N9",
  "N10",
  "N11",
  "N12",
  "CodexCli",
  "ManagedCodex",
  "AcpAgent",
  "McpCaller",
  "Inventory",
  "CodexClientUser",
  "ProviderSupervisor",
]);
const familySchema = z.enum(
  Object.keys(PRODUCER_CONSUMER_TABLE) as [MessageFamily, ...MessageFamily[]],
);
const counterpartSchema = z.union([
  z.strictObject({ kind: z.literal("Node"), node: partySchema }),
  z.strictObject({
    kind: z.enum([
      "CodexCli",
      "ManagedCodex",
      "AcpAgent",
      "McpCaller",
      "Inventory",
      "CodexClientUser",
      "ProviderSupervisor",
    ]),
  }),
]);
/** One record as `TapRecord` serializes it: its layer and intent are flattened tags. */
const recordSchema = z.strictObject({
  direction: z.enum(["consumed", "produced"]),
  family: familySchema,
  counterpart: counterpartSchema,
  reply: z.boolean(),
  kind: z.string(),
  layer: z.enum(["wireFrame", "body", "typedInput", "typedOutcome"]),
  body: z.looseObject({ root: z.string() }).optional(),
  connection: z.string(),
  intent: z.enum(["product", "deliberateInvalid", "syntheticApparatus"]),
  keyword: z.string().optional(),
  instancePath: z.string().optional(),
  detail: z.string().optional(),
  payload: z.unknown().optional(),
  raw: z.unknown().optional(),
  answers: z.string().optional(),
  conformance: z.looseObject({ result: z.string() }).optional(),
  setup: z.literal(true).optional(),
});
/** The schema side of a finished case (`SchemaSettlement`). */
const schemaSettlementSchema = z.looseObject({
  expected: z.array(z.unknown()),
  unlisted: z.array(z.unknown()),
  fixed: z.array(z.unknown()),
  coverageGaps: z.array(z.string()),
  declaredGaps: z.array(z.string()),
  injectionsNotShown: z.array(z.string()),
  apparatus: z.array(z.unknown()),
  unanswered: z.array(z.unknown()),
});
const outcomeSchema = z.looseObject({
  outcome: z.enum([
    "passed",
    "expectedFailure",
    "unexpectedFailure",
    "defectChanged",
    "defectNotListed",
    "unexpectedPass",
  ]),
});
export const tapRecordingSchema = z.looseObject({
  caseId: z.string(),
  node: partySchema,
  case: z.string(),
  catalogRow: z.string(),
  records: z.array(recordSchema),
  outcome: outcomeSchema,
  schema: schemaSettlementSchema,
});
export type TapRecording = z.infer<typeof tapRecordingSchema>;

function counterpartParty(counterpart: TapRecording["records"][number]["counterpart"]): Party {
  return counterpart.kind === "Node" ? counterpart.node : counterpart.kind;
}

export interface MessageFinding {
  readonly caseId: string;
  readonly detail: string;
}

/** The declared structural defect a recorded edge reproduces, if any. */
export function declaredDefectFor(
  node: Party,
  record: Pick<TapRecording["records"][number], "family" | "direction" | "reply" | "counterpart">,
) {
  const counterpart = counterpartParty(record.counterpart);
  return DECLARED_STRUCTURAL_DEFECTS.find(
    (defect) =>
      defect.node === node &&
      defect.family === record.family &&
      defect.counterpart === counterpart &&
      defect.direction === record.direction &&
      defect.reply === record.reply,
  );
}

/**
 * Checks one recorded message against the table row of its family. The sender must be a producer
 * (or intermediary) of the row and the receiver a consumer (or intermediary). A reply travels on
 * its request's edge in the other direction under the request's row, so for a reply the roles swap
 * (Program Design #producers-consumers, "Replies").
 */
export function tableFindings(recording: TapRecording): MessageFinding[] {
  const findings: MessageFinding[] = [];
  for (const record of recording.records) {
    const row = PRODUCER_CONSUMER_TABLE[record.family];
    const node = recording.node;
    const other = counterpartParty(record.counterpart);
    if (declaredDefectFor(node, record) !== undefined) continue;
    const sends = [...row.producers, ...row.intermediaries];
    const takes = [...row.consumers, ...row.intermediaries];
    const outbound = record.direction === "produced";
    const [sender, receiver] = outbound ? [node, other] : [other, node];
    const [rowSender, rowReceiver] = record.reply ? [receiver, sender] : [sender, receiver];
    const label = `${record.direction}${record.reply ? " reply" : ""} ${record.family} "${record.kind}" with ${other}`;
    if (!sends.includes(rowSender))
      findings.push({
        caseId: recording.caseId,
        detail: `${node} ${label}: ${rowSender} is not a producer of "${row.message}"`,
      });
    if (!takes.includes(rowReceiver))
      findings.push({
        caseId: recording.caseId,
        detail: `${node} ${label}: ${rowReceiver} is not a consumer of "${row.message}"`,
      });
  }
  return findings;
}

/** Why a case did not settle: its behavior outcome, and each unsettled part of its schema side. */
function unsettledDetails(recording: TapRecording): MessageFinding[] {
  const details: string[] = [];
  if (!["passed", "expectedFailure"].includes(recording.outcome.outcome))
    details.push(recording.outcome.outcome);
  const schema = recording.schema;
  if (schema.unlisted.length > 0) details.push(`${schema.unlisted.length} unlisted violations`);
  if (schema.fixed.length > 0) details.push(`${schema.fixed.length} listed defects now fixed`);
  if (schema.coverageGaps.length > 0) details.push(`${schema.coverageGaps.length} coverage gaps`);
  if (schema.injectionsNotShown.length > 0)
    details.push(`${schema.injectionsNotShown.length} injected violations not shown`);
  return details.map((detail) => ({ caseId: recording.caseId, detail }));
}

export interface NodeTapReport {
  readonly recordings: number;
  readonly missingRows: readonly string[];
  readonly unknownRows: readonly string[];
  readonly unsettled: readonly MessageFinding[];
  readonly expectedFailures: readonly string[];
  readonly tableFindings: readonly MessageFinding[];
  /** Recorded edges that reproduce a declared structural defect (see DECLARED_STRUCTURAL_DEFECTS). */
  readonly declaredDefectEdges: readonly MessageFinding[];
  /** Every distinct (node, direction, family, counterpart, kind) seen: the per-message view. */
  readonly messages: readonly string[];
}

export function buildReport(recordings: readonly TapRecording[]): NodeTapReport {
  const recordedRows = new Set(
    recordings.map((recording) => `${recording.node}: ${recording.catalogRow}`),
  );
  const required = new Set(REQUIRED_CATALOG_ROWS.map((row) => `${row.node}: ${row.row}`));
  const messages = new Set<string>();
  for (const recording of recordings)
    for (const record of recording.records)
      messages.add(
        `${recording.node} ${record.direction} ${record.family} ${counterpartParty(record.counterpart)} ${record.kind}`,
      );
  return {
    recordings: recordings.length,
    missingRows: [...required].filter((row) => !recordedRows.has(row)).sort(),
    unknownRows: [...recordedRows].filter((row) => !required.has(row)).sort(),
    unsettled: recordings.flatMap((recording) => unsettledDetails(recording)),
    expectedFailures: recordings
      .filter((recording) => recording.outcome.outcome === "expectedFailure")
      .map((recording) => recording.caseId)
      .sort(),
    tableFindings: recordings.flatMap(tableFindings),
    declaredDefectEdges: recordings.flatMap((recording) =>
      recording.records.flatMap((record) => {
        const defect = declaredDefectFor(recording.node, record);
        return defect === undefined
          ? []
          : [
              {
                caseId: recording.caseId,
                detail: `${defect.owner}: ${defect.defect} (${record.kind})`,
              },
            ];
      }),
    ),
    messages: [...messages].sort(),
  };
}

export function reportPasses(report: NodeTapReport): boolean {
  return (
    report.recordings > 0 &&
    report.missingRows.length === 0 &&
    report.unknownRows.length === 0 &&
    report.unsettled.length === 0 &&
    report.tableFindings.length === 0
  );
}

export function readRecordings(directory: string): TapRecording[] {
  const recordings: TapRecording[] = [];
  for (const node of readdirSync(directory).sort()) {
    const nodeDirectory = join(directory, node);
    if (!statSync(nodeDirectory).isDirectory()) continue;
    for (const file of readdirSync(nodeDirectory).sort()) {
      if (!file.endsWith(".json")) continue;
      const path = join(nodeDirectory, file);
      recordings.push(tapRecordingSchema.parse(JSON.parse(readFileSync(path, "utf8"))));
    }
  }
  return recordings;
}

export function main(argv: readonly string[] = process.argv.slice(2)): number {
  const [directory, flag, output] = argv;
  if (
    directory === undefined ||
    (flag !== undefined && (flag !== "--json" || output === undefined))
  ) {
    console.error(
      "Usage: node verification/tooling/node-taps/node-tap-report.ts <recording dir> [--json <out.json>]",
    );
    return 2;
  }
  const report = buildReport(readRecordings(directory));
  if (output !== undefined) writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`);
  console.log(`recordings: ${report.recordings}`);
  console.log(`declared gaps (not tapped in D0): ${DECLARED_GAPS.length}`);
  for (const gap of DECLARED_GAPS)
    console.log(`  - ${gap.node}: ${gap.row} — owner ${gap.owner}: ${gap.reason}`);
  console.log(`missing catalog rows: ${report.missingRows.length}`);
  for (const row of report.missingRows) console.log(`  - ${row}`);
  console.log(`recorded rows outside the inventory: ${report.unknownRows.length}`);
  for (const row of report.unknownRows) console.log(`  - ${row}`);
  console.log(`unsettled cases: ${report.unsettled.length}`);
  for (const finding of report.unsettled) console.log(`  - ${finding.caseId}: ${finding.detail}`);
  console.log(`expected failures: ${report.expectedFailures.length}`);
  for (const caseId of report.expectedFailures) console.log(`  - ${caseId}`);
  console.log(`declared structural defects reproduced: ${report.declaredDefectEdges.length}`);
  for (const edge of report.declaredDefectEdges) console.log(`  - ${edge.caseId}: ${edge.detail}`);
  for (const defect of DECLARED_STRUCTURAL_DEFECTS)
    for (const anchor of defect.anchors) console.log(`    anchor ${defect.owner}: ${anchor}`);
  console.log(`producer/consumer table findings: ${report.tableFindings.length}`);
  for (const finding of report.tableFindings)
    console.log(`  - ${finding.caseId}: ${finding.detail}`);
  return reportPasses(report) ? 0 : 1;
}

if (import.meta.main) process.exitCode = main();
