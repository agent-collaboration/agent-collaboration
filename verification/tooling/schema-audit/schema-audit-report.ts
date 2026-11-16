/**
 * `node verification/tooling/schema-audit/schema-audit-report.ts <tap dir> [--live <dir>] [--out <schema-audit.json>]`
 *
 * The D0 problem list (R53, R54, Program Design Y5). It joins, into one list sorted by who fixes
 * each finding:
 * - the part-test recordings the Rust taps write (`CODEX_ROUTER_NODE_TAP_DIR`), with each
 *   record's check result and each case's settlement: violations, owned disagreements, coverage
 *   gaps, apparatus findings and part defects;
 * - the live recordings' check results (`*.check.json` from `check_recording`) and their step
 *   outcomes (`*.steps.json`): violations and steps not exercised;
 * - the pinned definitions: protocols with no authoritative definition, a wrong pin, and every
 *   message kind no checked record contains (unseen kinds are counted from checked records only,
 *   never from kind labels);
 * - the source-coverage ledger (uncovered send sites) and the code inventory (duplicates, hand
 *   conversions, JSON carried as a string);
 * - the named live gap: managed Codex has no live wire coverage (addendum 2b).
 * It writes `schema-audit.json` (default `<tap dir>/schema-audit.json`) and exits 1 when any
 * finding names no one who fixes it.
 */
import { createHash } from "node:crypto";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { z } from "zod";

import { readRecordings } from "../node-taps/node-tap-report.ts";
import type { TapRecording } from "../node-taps/node-tap-report.ts";
import { DEFINITION_INVENTORY } from "./definition-inventory.ts";
import type {
  ContractOwner,
  Finding,
  FixedBy,
  InventoryRow,
  SendOrigin,
  LedgerSection,
  Part,
} from "./schema-audit-types.ts";
import { LEDGER_SECTIONS } from "./source-coverage-ledger.ts";

const repositoryRoot = join(import.meta.dirname, "../../..");
const schemas = join(repositoryRoot, "verification/schemas");

/** A finding whose fixer could not be named: the report fails while any exist. */
export interface UnownedFinding {
  readonly kind: Finding["kind"];
  readonly detail: string;
}

/** The Rust `ContractOwner` names, as the check results spell them. */
const rustOwner: Record<string, ContractOwner> = {
  codex: "codex",
  acp: "acp",
  claudeExtra: "claude-extra",
  routerExtension: "router-extension",
  cursorExtra: "cursor-extra",
  mcp: "mcp",
  routerVocabulary: "router-vocabulary",
};

/** Who fixes a finding found on a live recording, or anywhere no list entry names it: the edges
 * (D1) own upstream contracts; the inside (D2) owns the Router's own extensions and vocabulary. */
export function fixerForOwner(owner: ContractOwner): FixedBy {
  return owner === "router-extension" || owner === "router-vocabulary" ? "D2" : "D1";
}

/** The Rust `Deliverable` serialization, camelCase (`d1`, `d2`, `cliProject`, `{external}`). */
function fixerFromDeliverable(value: unknown): FixedBy | undefined {
  if (value === "d1") return "D1";
  if (value === "d2") return "D2";
  if (value === "cliProject") return "cli-project";
  if (typeof value === "object" && value !== null && "external" in value)
    return { external: String((value as { external: unknown }).external) };
  return undefined;
}

const conformanceSchema = z.looseObject({
  result: z.string(),
  owner: z.string().optional(),
  definition: z.string().optional(),
  method: z.string().optional(),
  violations: z
    .array(
      z.looseObject({
        schemaPath: z.string(),
        instancePath: z.string(),
        keyword: z.string(),
        detail: z.string(),
      }),
    )
    .optional(),
  /** A composite's parts: the standard verdict first, then each owned `_meta` place's. */
  parts: z.array(z.unknown()).optional(),
});
type Conformance = z.infer<typeof conformanceSchema>;

/** A verdict's leaves: a composite's parts, in order, flattened; any other verdict is its own. */
function leavesOf(conformance: Conformance): Conformance[] {
  if (conformance.result !== "composite") return [conformance];
  return (conformance.parts ?? []).flatMap((part) => {
    const parsed = conformanceSchema.safeParse(part);
    return parsed.success ? leavesOf(parsed.data) : [];
  });
}

/** The frame's standard verdict: the first leaf, which names the frame's owner and definition. */
function primaryOf(conformance: Conformance): Conformance {
  return leavesOf(conformance)[0] ?? conformance;
}

const defectSchema = z.looseObject({
  direction: z.string(),
  kind: z.string(),
  ordinal: z.number(),
  schemaPath: z.string(),
  instancePath: z.string(),
  keyword: z.string(),
  detail: z.string(),
  deliverable: z.unknown().optional(),
});

/** The protocol role of a message's sender: the client or the server (ACP: the agent). */
type SenderRole = "client" | "server";

/** One checked frame from anywhere: a part-test record or a live recording line. */
interface CheckedFrame {
  readonly where: string;
  /** The method; an MCP `tools/call` is `tools/call <tool>`, one kind per advertised tool. */
  readonly method: string | undefined;
  readonly sender: SenderRole;
  readonly conformance: Conformance;
  /** The tool names a captured MCP `tools/list` result advertised: N11's true MCP definition. */
  readonly advertisedTools?: readonly string[];
}

/** An MCP `tools/call` names its tool: each advertised tool is its own kind (R53). */
function kindMethod(method: string | undefined, tool: unknown): string | undefined {
  return method === "tools/call" && typeof tool === "string" ? `tools/call ${tool}` : method;
}

/**
 * Whether a frame is an MCP server's `tools/list` reply the checker paired with its request: its
 * standard verdict names `tools/list result` (mcp_definition.rs). A raw value merely shaped like
 * `result.tools` on another edge advertises nothing (correction 4, V4).
 */
function pairedAdvertisement(conformance: Conformance): boolean {
  const primary = primaryOf(conformance);
  return (
    primary.owner === "mcp" &&
    primary.definition === "tools/list result" &&
    ["conforms", "violations"].includes(primary.result)
  );
}

/**
 * The families on the MCP edge (the checker's `family_edge`): an advertisement counts only from a
 * server-sent `tools/list` reply on one of them (correction 5, C4-4). A tap's declared `answers`
 * pairing stays valid context; this sender and edge rule applies to it too.
 */
const MCP_FAMILIES: ReadonlySet<string> = new Set(["mcpToolCall", "mcpToolSchema"]);

/**
 * Limits of the checker the report states as information, not findings (correction 5, C4-5).
 */
export const NAMED_LIMITS: readonly string[] = [
  "MCP schema equality (mcp_definition.rs normalized_number): an integral float at or above 2^53 is not canonicalized, so 9007199254740992 and 9007199254740992.0 compare as different numbers. No pinned Codex literal reaches that range.",
];

/** The tool names a `tools/list` result frame advertises. */
function advertisedToolNames(raw: unknown): string[] {
  const tools = (raw as { result?: { tools?: unknown } } | null)?.result?.tools;
  return Array.isArray(tools)
    ? tools.flatMap((tool) =>
        typeof (tool as { name?: unknown })?.name === "string"
          ? [(tool as { name: string }).name]
          : [],
      )
    : [];
}

/**
 * Whether `node`, the part that recorded a message of `family`, plays the server on its edge:
 * the same table as the checker's `family_edge` (edge_conformance.rs). N8 serves the served-N8
 * edge, and any other part recording it is its client.
 */
function recorderIsServer(family: string, node: string): boolean {
  switch (family) {
    case "codexClientRequestFromCli":
    case "codexMessageToCli":
    case "acpServedForRouterSessions":
    case "mcpToolCall":
    case "mcpToolSchema":
      return true;
    case "acpRequestToCodexAdapter":
    case "acpServedForCodexSessions":
      return node === "N8";
    default:
      return false;
  }
}

function senderOf(family: string, node: string, direction: "consumed" | "produced"): SenderRole {
  return recorderIsServer(family, node) === (direction === "produced") ? "server" : "client";
}

function methodOf(raw: unknown): string | undefined {
  return typeof raw === "object" &&
    raw !== null &&
    typeof (raw as { method?: unknown }).method === "string"
    ? (raw as { method: string }).method
    : undefined;
}

function tapFrames(recordings: readonly TapRecording[]): CheckedFrame[] {
  return recordings.flatMap((recording) =>
    recording.records.flatMap((record): CheckedFrame[] => {
      const parsed = conformanceSchema.safeParse(record.conformance);
      return parsed.success
        ? [
            {
              where: recording.caseId,
              method: kindMethod(
                methodOf(record.raw),
                (record.raw as { params?: { name?: unknown } } | null)?.params?.name,
              ),
              sender: senderOf(record.family, recording.node, record.direction),
              conformance: parsed.data,
              advertisedTools:
                pairedAdvertisement(parsed.data) &&
                MCP_FAMILIES.has(record.family) &&
                senderOf(record.family, recording.node, record.direction) === "server"
                  ? advertisedToolNames(record.raw)
                  : [],
            },
          ]
        : [];
    }),
  );
}

const liveCheckSchema = z.looseObject({
  edge: z.string(),
  recording: z.string(),
  results: z.array(
    z.looseObject({
      seq: z.number(),
      direction: z.enum(["clientToServer", "serverToClient", "cliToServer", "serverToCli"]),
      method: z.string().nullable().optional(),
      /** An MCP `tools/call`'s tool name (check_recording writes it since correction 3). */
      tool: z.string().nullable().optional(),
      /** A paired MCP server `tools/list` reply's advertised tool names (since correction 4). */
      advertisedTools: z.array(z.string()).optional(),
      conformance: conformanceSchema,
    }),
  ),
  /** Requests no reply answered by the end of the recording (`ConnectionLedger::unanswered`). */
  unanswered: z.array(
    z.looseObject({ connection: z.string(), id: z.string(), method: z.string() }),
  ),
});
const liveStepsSchema = z.array(
  z.looseObject({
    name: z.string(),
    tool: z.string(),
    kind: z.enum(["exercised", "notExercised"]),
    reason: z.string().optional(),
  }),
);

export interface LiveInputs {
  readonly checks: ReadonlyArray<z.infer<typeof liveCheckSchema>>;
  readonly steps: ReadonlyArray<{
    readonly file: string;
    readonly outcomes: z.infer<typeof liveStepsSchema>;
  }>;
}

export function readLive(directory: string | undefined): LiveInputs {
  if (directory === undefined) return { checks: [], steps: [] };
  const files = readdirSync(directory).sort();
  return {
    checks: files
      .filter((file) => file.endsWith(".check.json"))
      .map((file) =>
        liveCheckSchema.parse(JSON.parse(readFileSync(join(directory, file), "utf8"))),
      ),
    steps: files
      .filter((file) => file.endsWith(".steps.json"))
      .map((file) => ({
        file,
        outcomes: liveStepsSchema.parse(JSON.parse(readFileSync(join(directory, file), "utf8"))),
      })),
  };
}

function liveFrames(live: LiveInputs): CheckedFrame[] {
  return live.checks.flatMap((check) =>
    check.results.map((result) => ({
      where: `${check.edge} ${check.recording}#${result.seq}`,
      method: kindMethod(result.method ?? undefined, result.tool),
      advertisedTools: result.advertisedTools ?? [],
      sender:
        result.direction.startsWith("cli") || result.direction.startsWith("client")
          ? ("client" as const)
          : ("server" as const),
      conformance: result.conformance,
    })),
  );
}

/** The owner the frames of a live edge are held to, for its coverage gaps. */
function edgeOwner(edge: string): ContractOwner {
  if (edge === "mcp") return "mcp";
  return edge.endsWith("codex") || edge.includes("Codex") ? "codex" : "acp";
}

/** Methods that only open a connection: a capture holding nothing else is no evidence of the edge. */
const OPENING_METHODS: ReadonlySet<string> = new Set([
  "initialize",
  "initialized",
  "notifications/initialized",
]);

/**
 * Coverage gaps of live recordings: each request no reply answered, and a recording that holds
 * only the opening exchange.
 */
function liveCoverageFindings(live: LiveInputs): Finding[] {
  return live.checks.flatMap((check): Finding[] => {
    const owner = edgeOwner(check.edge);
    const unanswered = check.unanswered.map((request): Finding => ({
      kind: "coverage-gap",
      owner,
      where: { edge: `${check.edge} ${check.recording}` },
      detail: `request ${request.method} (id ${request.id}) on connection ${request.connection} has no reply in the recording`,
      fixedBy: fixerForOwner(owner),
    }));
    const methods = check.results.flatMap((result) => (result.method ? [result.method] : []));
    const openingOnly = methods.every((method) => OPENING_METHODS.has(method));
    return openingOnly
      ? [
          ...unanswered,
          {
            kind: "coverage-gap",
            owner,
            where: { edge: `${check.edge} ${check.recording}` },
            detail:
              "the recording holds only the opening exchange: no evidence of the edge's own traffic",
            fixedBy: fixerForOwner(owner),
          },
        ]
      : unanswered;
  });
}

/** Violations and disagreements of checked frames no settled case accounts for (live frames). */
function frameFindings(frames: readonly CheckedFrame[]): Finding[] {
  return frames.flatMap((frame): Finding[] =>
    // Every component of a frame counts: a composite's owned `_meta` parts file their own findings.
    leavesOf(frame.conformance).flatMap((leaf): Finding[] => {
      const owner = rustOwner[leaf.owner ?? ""] ?? "acp";
      if (leaf.result === "violations")
        return (leaf.violations ?? []).map((violation) => ({
          kind: violation.keyword === "routerCodec" ? "owned-disagreement" : "violation",
          owner,
          where: { edge: frame.where },
          detail:
            `${leaf.definition ?? ""} ${violation.instancePath} ${violation.keyword} ${violation.detail}`.trim(),
          fixedBy: fixerForOwner(owner),
        }));
      if (leaf.result === "incomplete")
        return [
          {
            kind: "coverage-gap",
            owner,
            where: { edge: frame.where },
            detail: `cannot be judged: ${JSON.stringify(leaf["missing"])}`,
            fixedBy: fixerForOwner(owner),
          },
        ];
      return [];
    }),
  );
}

/** Each record's verdict, keyed as defects name records: direction, kind and ordinal. */
function recordVerdicts(recording: TapRecording): Map<string, Conformance> {
  const verdicts = new Map<string, Conformance>();
  const ordinals = new Map<string, number>();
  for (const record of recording.records) {
    const slot = `${record.direction} ${record.kind}`;
    const ordinal = ordinals.get(slot) ?? 0;
    ordinals.set(slot, ordinal + 1);
    const parsed = conformanceSchema.safeParse(record.conformance);
    if (parsed.success) verdicts.set(`${slot} #${ordinal}`, parsed.data);
  }
  return verdicts;
}

/**
 * A defect's owner: the owner of the leaf whose violations hold it (correction 4, V4). A frame's
 * owned `_meta` violation belongs to its owner (RouterExtension, ClaudeExtra), not to the frame's
 * standard verdict; with no matching leaf, the standard verdict's owner.
 */
function defectOwner(
  verdict: Conformance | undefined,
  defect: z.infer<typeof defectSchema>,
): ContractOwner | undefined {
  if (verdict === undefined) return undefined;
  const holder = leavesOf(verdict).find((leaf) =>
    (leaf.violations ?? []).some(
      (violation) =>
        violation.schemaPath === defect.schemaPath &&
        violation.instancePath === defect.instancePath &&
        violation.keyword === defect.keyword &&
        violation.detail === defect.detail,
    ),
  );
  return rustOwner[(holder ?? primaryOf(verdict)).owner ?? ""];
}

/** Findings from the part-test case settlements, with the fixer each list entry names. */
function caseFindings(recordings: readonly TapRecording[], unowned: UnownedFinding[]): Finding[] {
  const findings: Finding[] = [];
  for (const recording of recordings) {
    const part = recording.node as Part;
    const where = { part, edge: recording.caseId };
    const verdicts = recordVerdicts(recording);
    const ownerOf = (defect: z.infer<typeof defectSchema>): ContractOwner =>
      defectOwner(verdicts.get(`${defect.direction} ${defect.kind} #${defect.ordinal}`), defect) ??
      "router-vocabulary";
    for (const entry of recording.schema.expected) {
      const defect = defectSchema.parse(entry);
      const fixedBy = fixerFromDeliverable(defect.deliverable);
      const kind = defect.keyword === "routerCodec" ? "owned-disagreement" : "violation";
      const detail =
        `${defect.direction} ${defect.kind} #${defect.ordinal} ${defect.instancePath} ${defect.keyword} ${defect.detail}`.trim();
      if (fixedBy === undefined) unowned.push({ kind, detail: `${recording.caseId}: ${detail}` });
      else findings.push({ kind, owner: ownerOf(defect), where, detail, fixedBy });
    }
    for (const entry of recording.schema.unlisted) {
      const defect = defectSchema.parse(entry);
      unowned.push({
        kind: "violation",
        detail: `${recording.caseId}: unlisted ${defect.kind} #${defect.ordinal} ${defect.instancePath} ${defect.keyword}`,
      });
    }
    for (const request of recording.schema.unanswered) {
      const pending = z
        .looseObject({ connection: z.string(), id: z.string(), method: z.string() })
        .parse(request);
      findings.push({
        kind: "coverage-gap",
        owner: "router-vocabulary",
        where,
        detail: `request ${pending.method} (id ${pending.id}) on connection ${pending.connection} has no reply in the case's capture`,
        fixedBy: "D1",
      });
    }
    for (const gap of [...recording.schema.coverageGaps, ...recording.schema.declaredGaps])
      findings.push({
        kind: "coverage-gap",
        owner: "router-vocabulary",
        where,
        detail: gap,
        fixedBy: "D1",
      });
    for (const entry of recording.schema.apparatus) {
      const defect = defectSchema.parse(entry);
      findings.push({
        kind: "apparatus",
        owner: ownerOf(defect),
        where,
        detail: `${defect.kind} #${defect.ordinal} ${defect.instancePath} ${defect.keyword}`,
        fixedBy: "D1",
      });
    }
    const outcome = recording.outcome as {
      outcome: string;
      deliverable?: unknown;
      defect?: unknown;
    };
    if (outcome.outcome === "expectedFailure") {
      const fixedBy = fixerFromDeliverable(outcome.deliverable);
      const detail = String(outcome.defect ?? "");
      if (fixedBy === undefined)
        unowned.push({ kind: "part-defect", detail: `${recording.caseId}: ${detail}` });
      else
        findings.push({ kind: "part-defect", owner: "router-vocabulary", where, detail, fixedBy });
    }
  }
  return findings;
}

/** One message kind of a pinned definition: who owns it, which role sends it, and its method. */
export interface PinnedKind {
  readonly owner: ContractOwner;
  readonly sender: SenderRole;
  readonly method: string;
}

function kindKey(kind: PinnedKind): string {
  return `${kind.owner}|${kind.sender}|${kind.method}`;
}

/**
 * Each message kind of each pinned definition (R53), keyed by owner, sending role and method,
 * so a method name one protocol or one direction shares with another is never counted for both.
 */
export function pinnedKinds(
  advertisedTools: readonly string[] = [],
): Record<string, readonly PinnedKind[]> {
  const bundle = JSON.parse(
    readFileSync(join(schemas, "codex-app-server-0.166.0.experimental.json"), "utf8"),
  ) as {
    definitions: Record<string, { oneOf: Array<{ properties: { method: { enum: [string] } } }> }>;
  };
  const codex = (union: string, sender: SenderRole): PinnedKind[] =>
    (bundle.definitions[union]?.oneOf ?? []).map((variant) => ({
      owner: "codex",
      sender,
      method: variant.properties.method.enum[0],
    }));
  const acp = JSON.parse(readFileSync(join(schemas, "acp-schema-1.9.1.json"), "utf8")) as {
    $defs: Record<string, { "x-method"?: string; "x-side"?: string }>;
  };
  // `x-side` names the handler; the sender is the other side ("protocol" travels both ways).
  const acpKinds = (suffix: string): PinnedKind[] =>
    Object.entries(acp.$defs).flatMap(([name, definition]): PinnedKind[] => {
      const method = definition["x-method"];
      if (!name.endsWith(suffix) || method === undefined) return [];
      const senders: SenderRole[] =
        definition["x-side"] === "agent"
          ? ["client"]
          : definition["x-side"] === "client"
            ? ["server"]
            : ["client", "server"];
      return senders.map((sender) => ({ owner: "acp", sender, method }));
    });
  return {
    "codex ClientRequest": codex("ClientRequest", "client"),
    "codex ServerRequest": codex("ServerRequest", "server"),
    "codex ServerNotification": codex("ServerNotification", "server"),
    "codex ClientNotification": codex("ClientNotification", "client"),
    "acp requests": acpKinds("Request"),
    "acp notifications": acpKinds("Notification"),
    "claude extras": claudeKinds(),
    "mcp n11": mcpKinds(advertisedTools),
  };
}

/** Claude's kinds, from the extras pin: its `x-methods` (the client sends) and `x-notifications`
 * (each with its declared sender). */
function claudeKinds(): PinnedKind[] {
  const pin = JSON.parse(
    readFileSync(join(schemas, "claude-agent-acp-0.81.0.extensions.json"), "utf8"),
  ) as {
    "x-methods"?: Record<string, unknown>;
    "x-notifications"?: Record<string, { sender?: string }>;
  };
  return [
    ...Object.keys(pin["x-methods"] ?? {}).map((method): PinnedKind => ({
      owner: "claude-extra",
      sender: "client",
      method,
    })),
    ...Object.entries(pin["x-notifications"] ?? {}).map(([method, entry]): PinnedKind => ({
      owner: "claude-extra",
      sender: entry.sender === "agent" ? "server" : "client",
      method,
    })),
  ];
}

/** The rmcp methods N11 serves (mcp_server.rs:296-352 `initialize`, `list_tools`, `call_tool`, and
 * rmcp's default `ping`), with `notifications/initialized`, and one `tools/call <tool>` kind per
 * tool N11's captured `tools/list` advertised. */
function mcpKinds(advertisedTools: readonly string[]): PinnedKind[] {
  const served = ["initialize", "notifications/initialized", "tools/list", "ping"];
  return [
    ...served.map((method): PinnedKind => ({ owner: "mcp", sender: "client", method })),
    ...[...new Set(advertisedTools)].sort().map((tool): PinnedKind => ({
      owner: "mcp",
      sender: "client",
      method: `tools/call ${tool}`,
    })),
  ];
}

/** Every tool any captured `tools/list` result advertised. */
function advertisedTools(frames: readonly CheckedFrame[]): string[] {
  return frames.flatMap((frame) => frame.advertisedTools ?? []);
}

/** The kinds checked frames actually carried: a frame counts only when the checker judged it. */
export function seenKinds(frames: readonly CheckedFrame[]): Set<string> {
  return new Set(
    frames
      // Seen credit only for a frame every leaf of which was judged: a composite holding an
      // Incomplete leaf (a call to an unadvertised tool, a missing owner) earns none (V4).
      .filter((frame) =>
        leavesOf(frame.conformance).every((leaf) =>
          ["conforms", "violations"].includes(leaf.result),
        ),
      )
      .flatMap((frame) => {
        const owner = rustOwner[primaryOf(frame.conformance).owner ?? ""];
        return frame.method === undefined || owner === undefined
          ? []
          : [kindKey({ owner, sender: frame.sender, method: frame.method })];
      }),
  );
}

/**
 * Who fixes an unseen kind: the edges whose types D1 owns (Codex, ACP, Claude's extras, MCP).
 * Any other owner names no fixer, so the ownership gate reports it rather than inventing one.
 */
export function unseenKindFixer(owner: ContractOwner): FixedBy | undefined {
  return owner === "codex" || owner === "acp" || owner === "claude-extra" || owner === "mcp"
    ? "D1"
    : undefined;
}

/**
 * One informational finding per unseen kind (R53, R54): placed by definition, role and method.
 * An unseen kind is never a failure; a kind no fixer owns is reported as unowned.
 */
export function unseenKindFindings(
  frames: readonly CheckedFrame[],
  definitions: Record<string, readonly PinnedKind[]> = pinnedKinds(),
  unowned: UnownedFinding[] = [],
): Finding[] {
  const seen = seenKinds(frames);
  return Object.entries(definitions).flatMap(([definition, kinds]) =>
    kinds
      .filter((kind) => !seen.has(kindKey(kind)))
      .flatMap((kind): Finding[] => {
        const detail = `${definition}: ${kind.sender} ${kind.method} is in the pinned definition but no checked record carries it`;
        const fixedBy = unseenKindFixer(kind.owner);
        if (fixedBy === undefined) {
          unowned.push({ kind: "unseen-kind", detail });
          return [];
        }
        return [
          {
            kind: "unseen-kind",
            owner: kind.owner,
            where: { definition, role: kind.sender, method: kind.method },
            detail,
            fixedBy,
          },
        ];
      }),
  );
}

/** Each definition's kinds no checked frame carried, as `sender:method`. */
export function unseenKinds(frames: readonly CheckedFrame[]): Record<string, string[]> {
  const seen = seenKinds(frames);
  return Object.fromEntries(
    Object.entries(pinnedKinds(advertisedTools(frames))).map(([definition, kinds]) => [
      definition,
      kinds
        .filter((kind) => !seen.has(kindKey(kind)))
        .map((kind) => `${kind.sender}:${kind.method}`),
    ]),
  );
}

function sha256(path: string): string {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function pinFindings(): Finding[] {
  const pins = JSON.parse(readFileSync(join(schemas, "pins.json"), "utf8")) as {
    pins: Array<{ protocol: string; sha256: string }>;
    noTrueDefinition: Array<{ protocol: string; reason: string }>;
  };
  const findings: Finding[] = pins.noTrueDefinition.map((entry) => ({
    kind: "no-authoritative-definition",
    owner: "cursor-extra",
    where: { file: "verification/schemas/pins.json" },
    detail: entry.reason,
    fixedBy: "D1",
  }));
  const vendored = "protocol-schemas/acp-protocol/stable-schema.json";
  const pinnedAcp = pins.pins.find((pin) => pin.protocol === "acp")?.sha256;
  const found = sha256(join(repositoryRoot, vendored));
  if (found !== pinnedAcp)
    findings.push({
      kind: "wrong-pin",
      owner: "acp",
      where: { file: vendored },
      detail: `the vendored ACP schema (sha256 ${found.slice(0, 12)}…) is not the schema of the ACP library the Router compiles (pinned ${String(pinnedAcp).slice(0, 12)}…)`,
      fixedBy: "D1",
    });
  return findings;
}

/**
 * A boundary encoding is the one legitimate encoding at its boundary and yields no finding, even
 * when it carries JSON in a string (an opaque cursor, a portable export). A duplicate or a
 * malformed site yields one finding, with its fixer.
 */
export function inventoryFindings(rows: readonly InventoryRow[]): Finding[] {
  return rows.flatMap((row): Finding[] => {
    if (row.disposition === "boundary-encoding") return [];
    const kind: Finding["kind"] =
      row.disposition === "duplicate"
        ? "duplicate"
        : row.kind === "json-in-string"
          ? "json-in-string"
          : "hand-conversion";
    return [
      {
        kind,
        owner: row.owner,
        where: { file: row.file, line: row.line },
        detail: `${row.defines}${row.duplicateGroup === null ? "" : ` [${row.duplicateGroup}]`}`,
        fixedBy: row.fixedBy,
      },
    ];
  });
}

/**
 * An uncovered product send is a coverage gap; an uncovered apparatus send (a fixture or driver
 * playing a counterparty) is an apparatus finding, never a gap in Router product coverage.
 */
export function ledgerFindings(sections: readonly LedgerSection[]): Finding[] {
  return sections.flatMap((section) =>
    section.sendSites.flatMap((site): Finding[] =>
      site.coverage.kind === "uncovered"
        ? [
            {
              kind: site.origin === "apparatus" ? "apparatus" : "coverage-gap",
              owner: "router-vocabulary",
              where: {
                edge: site.edge,
                file: site.file,
                line: site.line,
                ...("crate" in site.owner
                  ? { crate: site.owner.crate }
                  : { part: site.owner.part }),
              },
              detail: `send site ${site.sends} uncovered: ${site.coverage.reason}`,
              fixedBy:
                site.edge === "mcp" || site.edge.startsWith("acp") || site.edge.endsWith("codex")
                  ? "D1"
                  : "D2",
            },
          ]
        : [],
    ),
  );
}

const MANAGED_CODEX_LIVE_GAP: Finding = {
  kind: "coverage-gap",
  owner: "codex",
  where: { edge: "managed-codex" },
  detail:
    "no live wire coverage: the Host's one app_server_socket is both the --listen path and the Router's connection (app_server_launch.rs:77-80, collaboration_lifecycle.rs:75) and it refuses a pre-listening socket (lifecycle_owner.rs:319-326); the N10/N8 fake-server captures are this edge's wire evidence",
  fixedBy: "D1",
};

function liveStepFindings(live: LiveInputs): Finding[] {
  return live.steps.flatMap(({ file, outcomes }) =>
    outcomes
      .filter((outcome) => outcome.kind === "notExercised")
      .map((outcome): Finding => ({
        kind: "not-exercised",
        owner: "mcp",
        where: { edge: `mcp ${file}` },
        detail: `${outcome.name} (${outcome.tool}): ${outcome.reason ?? ""}`,
        fixedBy: "D1",
      })),
  );
}

export function sendSiteTotals(
  sections: readonly LedgerSection[],
): Record<SendOrigin, { covered: number; uncovered: number }> {
  const totals: Record<SendOrigin, { covered: number; uncovered: number }> = {
    product: { covered: 0, uncovered: 0 },
    apparatus: { covered: 0, uncovered: 0 },
  };
  for (const site of sections.flatMap((section) => section.sendSites))
    totals[site.origin][site.coverage.kind === "uncovered" ? "uncovered" : "covered"] += 1;
  return totals;
}

function fixerLabel(fixedBy: FixedBy): string {
  return typeof fixedBy === "string" ? fixedBy : `external:${fixedBy.external}`;
}

export interface SchemaAudit {
  readonly findings: readonly Finding[];
  readonly unowned: readonly UnownedFinding[];
  readonly unseenKinds: Record<string, string[]>;
  /** Checker limits stated as information, not findings. */
  readonly namedLimits: readonly string[];
  readonly totals: {
    readonly byKind: Record<string, number>;
    readonly byFixer: Record<string, number>;
    /** Send sites by origin, covered and uncovered, so apparatus never inflates product coverage. */
    readonly sendSites: Record<SendOrigin, { covered: number; uncovered: number }>;
  };
}

export function buildAudit(recordings: readonly TapRecording[], live: LiveInputs): SchemaAudit {
  const unowned: UnownedFinding[] = [];
  const frames = [...tapFrames(recordings), ...liveFrames(live)];
  const findings = [
    ...caseFindings(recordings, unowned),
    ...frameFindings(liveFrames(live)),
    ...liveCoverageFindings(live),
    ...liveStepFindings(live),
    ...pinFindings(),
    ...inventoryFindings(DEFINITION_INVENTORY),
    ...ledgerFindings(LEDGER_SECTIONS),
    ...unseenKindFindings(frames, pinnedKinds(advertisedTools(frames)), unowned),
    MANAGED_CODEX_LIVE_GAP,
  ].sort(
    (left, right) =>
      fixerLabel(left.fixedBy).localeCompare(fixerLabel(right.fixedBy)) ||
      left.kind.localeCompare(right.kind),
  );
  const unseen = unseenKinds(frames);
  const count = (labels: string[]): Record<string, number> =>
    labels.reduce<Record<string, number>>(
      (totals, label) => ({ ...totals, [label]: (totals[label] ?? 0) + 1 }),
      {},
    );
  return {
    findings,
    unowned,
    namedLimits: NAMED_LIMITS,
    unseenKinds: unseen,
    totals: {
      byKind: count(findings.map((finding) => finding.kind)),
      byFixer: count(findings.map((finding) => fixerLabel(finding.fixedBy))),
      sendSites: sendSiteTotals(LEDGER_SECTIONS),
    },
  };
}

export function main(argv: readonly string[] = process.argv.slice(2)): number {
  const [tapDirectory, ...rest] = argv;
  const flag = (name: string): string | undefined => {
    const index = rest.indexOf(name);
    return index < 0 ? undefined : rest[index + 1];
  };
  if (tapDirectory === undefined) {
    console.error(
      "usage: schema-audit-report.ts <tap dir> [--live <dir>] [--out <schema-audit.json>]",
    );
    return 2;
  }
  const audit = buildAudit(readRecordings(tapDirectory), readLive(flag("--live")));
  const out = flag("--out") ?? join(tapDirectory, "schema-audit.json");
  writeFileSync(out, `${JSON.stringify(audit, null, 2)}\n`);
  console.log(`findings: ${audit.findings.length}`);
  for (const [kind, total] of Object.entries(audit.totals.byKind).sort())
    console.log(`  ${kind}: ${total}`);
  for (const [fixer, total] of Object.entries(audit.totals.byFixer).sort())
    console.log(`  fixed by ${fixer}: ${total}`);
  for (const [origin, { covered, uncovered }] of Object.entries(audit.totals.sendSites))
    console.log(`  ${origin} send sites: ${covered} covered, ${uncovered} uncovered`);
  console.log(`unowned findings: ${audit.unowned.length}`);
  console.log(`named limits (information, not findings): ${audit.namedLimits.length}`);
  for (const limit of audit.namedLimits) console.log(`  - ${limit}`);
  for (const finding of audit.unowned) console.log(`  - ${finding.kind}: ${finding.detail}`);
  console.log(`wrote ${out}`);
  return audit.unowned.length === 0 ? 0 : 1;
}

if (import.meta.main) process.exitCode = main();
