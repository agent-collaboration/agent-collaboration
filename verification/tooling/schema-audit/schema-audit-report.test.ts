import assert from "node:assert/strict";
import { test } from "node:test";

import type { TapRecording } from "../node-taps/node-tap-report.ts";
import {
  buildAudit,
  inventoryFindings,
  pinnedKinds,
  ledgerFindings,
  sendSiteTotals,
  unseenKindFindings,
} from "./schema-audit-report.ts";
import type { InventoryRow, LedgerSection, SendSite } from "./schema-audit-types.ts";
import type { UnownedFinding } from "./schema-audit-report.ts";

const NO_LIVE = { checks: [], steps: [] };

function settled(): TapRecording["schema"] {
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

function recording(overrides: Partial<TapRecording>): TapRecording {
  return {
    caseId: "N1/case",
    node: "N1",
    case: "case",
    catalogRow: "row",
    records: [],
    outcome: { outcome: "passed" },
    schema: settled(),
    ...overrides,
  };
}

function defect(deliverable?: unknown): Record<string, unknown> {
  return {
    direction: "consumed",
    kind: "turn/start",
    ordinal: 0,
    schemaPath: "/x",
    instancePath: "/params",
    keyword: "required",
    detail: "threadId",
    ...(deliverable === undefined ? {} : { deliverable }),
  };
}

function turnStartRecord(
  conformance: { result: string; [key: string]: unknown } | undefined,
): TapRecording["records"][number] {
  return {
    direction: "consumed",
    family: "codexClientRequestFromCli",
    counterpart: { kind: "CodexCli" },
    reply: false,
    kind: "turn/start",
    layer: "wireFrame",
    connection: "case",
    intent: "product",
    raw: { id: 1, method: "turn/start", params: {} },
    ...(conformance === undefined ? {} : { conformance }),
  };
}

test("a listed violation becomes a finding with the fixer its entry names", () => {
  const audit = buildAudit(
    [recording({ schema: { ...settled(), expected: [defect("d1")] } })],
    NO_LIVE,
  );
  const violations = audit.findings.filter((finding) => finding.kind === "violation");
  assert.equal(violations.length, 1);
  assert.equal(violations[0]?.fixedBy, "D1");
  assert.deepEqual(audit.unowned, []);
});

test("a finding with no fixer is reported as unowned, never dropped", () => {
  const audit = buildAudit(
    [recording({ schema: { ...settled(), expected: [defect()], unlisted: [defect()] } })],
    NO_LIVE,
  );
  assert.equal(audit.unowned.length, 2);
});

test("unseen kinds are counted from checked records, never from kind labels", () => {
  const labelledOnly = buildAudit([recording({ records: [turnStartRecord(undefined)] })], NO_LIVE);
  assert.ok(labelledOnly.unseenKinds["codex ClientRequest"]?.includes("client:turn/start"));
  const checked = buildAudit(
    [recording({ records: [turnStartRecord({ result: "conforms", owner: "codex" })] })],
    NO_LIVE,
  );
  assert.ok(!checked.unseenKinds["codex ClientRequest"]?.includes("client:turn/start"));
  assert.equal(checked.unseenKinds["codex ClientRequest"]?.length, 178);
});

test("coverage gaps and apparatus findings are separate kinds", () => {
  const audit = buildAudit(
    [
      recording({
        schema: { ...settled(), coverageGaps: ["consumed x #0: no raw"], apparatus: [defect()] },
      }),
    ],
    NO_LIVE,
  );
  const kinds = new Set(audit.findings.map((finding) => finding.kind));
  assert.ok(kinds.has("coverage-gap"));
  assert.ok(kinds.has("apparatus"));
  assert.ok(
    !audit.findings.some((finding) => finding.kind === "violation"),
    "apparatus is never a product violation",
  );
});

test("the pins, the inventory and the named live gap reach the list", () => {
  const audit = buildAudit([], NO_LIVE);
  const kinds = new Set(audit.findings.map((finding) => finding.kind));
  for (const kind of ["no-authoritative-definition", "wrong-pin", "duplicate"] as const)
    assert.ok(kinds.has(kind), kind);
  assert.ok(
    audit.findings.some(
      (finding) => finding.where.edge === "managed-codex" && finding.kind === "coverage-gap",
    ),
  );
});

test("a live step not exercised is its own finding", () => {
  const audit = buildAudit([], {
    checks: [],
    steps: [
      {
        file: "mcp.jsonl.steps.json",
        outcomes: [
          { name: "nativeControl", tool: "x", kind: "notExercised", reason: "not advertised" },
        ],
      },
    ],
  });
  assert.equal(audit.findings.filter((finding) => finding.kind === "not-exercised").length, 1);
});

function frameRecord(
  family: TapRecording["records"][number]["family"],
  direction: "consumed" | "produced",
  method: string,
  owner: string,
): TapRecording["records"][number] {
  return {
    direction,
    family,
    counterpart: { kind: "CodexCli" },
    reply: false,
    kind: method,
    layer: "wireFrame",
    connection: "case",
    intent: "product",
    raw: { jsonrpc: "2.0", id: 1, method, params: {} },
    conformance: { result: "conforms", owner },
  };
}

test("a method seen on one protocol or role never counts for another", () => {
  // N11 consumes an MCP initialize from its caller; N1 produces (as the server) a turn/start.
  const audit = buildAudit(
    [
      recording({
        node: "N11",
        records: [
          frameRecord("mcpToolCall", "consumed", "initialize", "mcp"),
          frameRecord("codexMessageToCli", "produced", "turn/start", "codex"),
        ],
      }),
    ],
    NO_LIVE,
  );
  assert.ok(audit.unseenKinds["codex ClientRequest"]?.includes("client:initialize"));
  assert.ok(audit.unseenKinds["acp requests"]?.includes("client:initialize"));
  assert.ok(audit.unseenKinds["codex ClientRequest"]?.includes("client:turn/start"));
});

function liveCheck(
  methods: string[],
  unanswered: Array<{ connection: string; id: string; method: string }>,
) {
  return {
    edge: "mcp",
    recording: "mcp.jsonl",
    results: methods.map((method, seq) => ({
      seq,
      direction: "clientToServer" as const,
      method,
      conformance: { result: "conforms", owner: "mcp" },
    })),
    unanswered,
  };
}

test("a live request left without its reply is a named coverage gap", () => {
  const audit = buildAudit([], {
    checks: [
      liveCheck(["initialize", "tools/list"], [{ connection: "0", id: "1", method: "tools/list" }]),
    ],
    steps: [],
  });
  const gaps = audit.findings.filter(
    (finding) => finding.kind === "coverage-gap" && finding.where.edge === "mcp mcp.jsonl",
  );
  assert.deepEqual(
    gaps.map((gap) => gap.detail),
    ["request tools/list (id 1) on connection 0 has no reply in the recording"],
  );
});

test("a live recording holding only the opening exchange is not evidence of the edge", () => {
  const opening = buildAudit([], {
    checks: [liveCheck(["initialize", "notifications/initialized"], [])],
    steps: [],
  });
  assert.ok(
    opening.findings.some((finding) => finding.detail.includes("only the opening exchange")),
  );
  const complete = buildAudit([], {
    checks: [liveCheck(["initialize", "tools/list"], [])],
    steps: [],
  });
  assert.ok(
    !complete.findings.some((finding) => finding.detail.includes("only the opening exchange")),
  );
});

test("a part-test request left unanswered is a coverage gap", () => {
  const audit = buildAudit(
    [
      recording({
        schema: {
          ...settled(),
          unanswered: [{ connection: "case", id: "7", method: "item/tool/requestUserInput" }],
        },
      }),
    ],
    NO_LIVE,
  );
  assert.ok(
    audit.findings.some(
      (finding) =>
        finding.kind === "coverage-gap" && finding.detail.includes("item/tool/requestUserInput"),
    ),
  );
});

test("every unseen kind is an owned finding placed by definition, role and method", () => {
  const audit = buildAudit([], NO_LIVE);
  const unseenRows = audit.findings.filter((finding) => finding.kind === "unseen-kind");
  const listed = Object.values(audit.unseenKinds).flat().length;
  assert.equal(unseenRows.length, listed);
  assert.ok(unseenRows.length > 0);
  assert.ok(
    unseenRows.every(
      (row) => row.fixedBy === "D1" && row.where.definition && row.where.role && row.where.method,
    ),
  );
  assert.ok(
    unseenRows.some(
      (row) =>
        row.where.definition === "codex ClientRequest" &&
        row.where.method === "turn/start" &&
        row.where.role === "client",
    ),
  );
});

test("an unseen kind no fixer owns is reported as unowned, not dropped", () => {
  const unowned: UnownedFinding[] = [];
  const rows = unseenKindFindings(
    [],
    { "router vocabulary": [{ owner: "router-vocabulary", sender: "client", method: "x/y" }] },
    unowned,
  );
  assert.deepEqual(rows, []);
  assert.equal(unowned.length, 1);
  assert.equal(unowned[0]?.kind, "unseen-kind");
});

const jsonInString: Pick<
  InventoryRow,
  "file" | "line" | "owner" | "kind" | "defines" | "usedBy" | "duplicateGroup"
> = {
  file: "crates/example/src/cursor.rs",
  line: 8,
  owner: "router-vocabulary",
  kind: "json-in-string",
  defines: "opaque page cursor, base64url JSON",
  usedBy: [{ part: "N11" }],
  duplicateGroup: "page-cursor",
};

test("a boundary encoding yields no finding; a duplicate yields one with its fixer", () => {
  const boundary: InventoryRow = { ...jsonInString, disposition: "boundary-encoding" };
  const duplicate: InventoryRow = { ...jsonInString, disposition: "duplicate", fixedBy: "D2" };
  const malformed: InventoryRow = { ...jsonInString, disposition: "malformed", fixedBy: "D2" };

  const fromBoundary = inventoryFindings([boundary]);
  const fromDuplicate = inventoryFindings([duplicate]);
  const fromMalformed = inventoryFindings([malformed]);

  assert.deepEqual(fromBoundary, []);
  assert.equal(fromDuplicate.length, 1);
  assert.equal(fromDuplicate[0]?.kind, "duplicate");
  assert.equal(fromDuplicate[0]?.fixedBy, "D2");
  assert.equal(fromMalformed.length, 1);
  assert.equal(fromMalformed[0]?.kind, "json-in-string");
});

function sendSite(origin: SendSite["origin"], uncovered: boolean): SendSite {
  return {
    file: "crates/example/src/send.rs",
    line: 1,
    edge: "mcp",
    sends: "tools/call",
    owner: { crate: "example" },
    origin,
    coverage: uncovered
      ? { kind: "uncovered", reason: "no kept capture" }
      : { kind: "live", recording: "mcp.json" },
  };
}

test("an uncovered apparatus send is an apparatus finding, counted apart from product sends", () => {
  const sections: LedgerSection[] = [
    {
      root: "crates/example",
      files: [],
      sendSites: [
        sendSite("product", true),
        sendSite("apparatus", true),
        sendSite("apparatus", false),
      ],
    },
  ];

  const kinds = ledgerFindings(sections).map((finding) => finding.kind);
  const totals = sendSiteTotals(sections);

  assert.deepEqual(kinds, ["coverage-gap", "apparatus"]);
  assert.deepEqual(totals, {
    product: { covered: 0, uncovered: 1 },
    apparatus: { covered: 1, uncovered: 1 },
  });
});

test("Claude's and MCP's kinds come from their definitions, with sender roles", () => {
  // Expected lists written from the Claude pin and N11's served methods, not from pinnedKinds.
  const kinds = pinnedKinds(["session_inspect", "message_send"]);
  const listed = (definition: string) =>
    (kinds[definition] ?? []).map((kind) => `${kind.owner} ${kind.sender} ${kind.method}`).sort();

  assert.deepEqual(listed("claude extras"), [
    "claude-extra client _session/goal",
    "claude-extra client _session/steering",
    "claude-extra server _auth/status_update",
  ]);
  assert.deepEqual(listed("mcp n11"), [
    "mcp client initialize",
    "mcp client notifications/initialized",
    "mcp client ping",
    "mcp client tools/call message_send",
    "mcp client tools/call session_inspect",
    "mcp client tools/list",
  ]);
});

test("an empty capture reports the pinned Claude notification as unseen", () => {
  const findings = unseenKindFindings([]);

  assert.ok(
    findings.some(
      (finding) =>
        finding.owner === "claude-extra" &&
        finding.where.role === "server" &&
        finding.where.method === "_auth/status_update",
    ),
  );
});

test("a tools/call is seen per advertised tool, and a composite frame files every part", () => {
  const tools = {
    ...frameRecord("mcpToolCall", "produced", "tools/list", "mcp"),
    raw: { jsonrpc: "2.0", id: 1, result: { tools: [{ name: "a" }, { name: "b" }] } },
    conformance: { result: "conforms", owner: "mcp", definition: "tools/list result" },
  };
  const callA = {
    ...frameRecord("mcpToolCall", "consumed", "tools/call", "mcp"),
    raw: { jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: "a" } },
  };
  const composite = {
    edge: "acp-agent",
    recording: "acp.jsonl",
    results: [
      {
        seq: 0,
        direction: "serverToClient" as const,
        method: null,
        conformance: {
          result: "composite",
          parts: [
            { result: "conforms", owner: "acp" },
            {
              result: "violations",
              owner: "routerExtension",
              definition: "ModelReasoningMeta",
              violations: [
                {
                  schemaPath: "ModelReasoningMeta",
                  instancePath: "/result/_meta/codexRouter/modelReasoning",
                  keyword: "routerCodec",
                  detail: "zodVsRust",
                },
              ],
            },
          ],
        },
      },
    ],
    unanswered: [],
  };

  const audit = buildAudit([recording({ node: "N11", records: [tools, callA] })], {
    checks: [composite],
    steps: [],
  });

  assert.ok(!audit.unseenKinds["mcp n11"]?.includes("client:tools/call a"));
  assert.ok(audit.unseenKinds["mcp n11"]?.includes("client:tools/call b"));
  assert.ok(
    audit.findings.some(
      (finding) =>
        finding.kind === "owned-disagreement" &&
        finding.detail.includes("/result/_meta/codexRouter/modelReasoning"),
    ),
  );
});

test("advertisements come only from paired MCP tools/list replies, live ones included", () => {
  const sentinel = {
    ...frameRecord("codexMessageToCli", "produced", "thread/list", "codex"),
    raw: { jsonrpc: "2.0", id: 1, result: { tools: [{ name: "__sentinel" }] } },
  };
  const live = {
    edge: "mcp",
    recording: "mcp.jsonl",
    results: [
      {
        seq: 0,
        direction: "serverToClient" as const,
        method: null,
        advertisedTools: ["live_only_tool"],
        conformance: { result: "conforms", owner: "mcp", definition: "tools/list result" },
      },
    ],
    unanswered: [],
  };

  const audit = buildAudit([recording({ node: "N1", records: [sentinel] })], {
    checks: [live],
    steps: [],
  });

  const mcp = audit.unseenKinds["mcp n11"] ?? [];
  assert.ok(!mcp.includes("client:tools/call __sentinel"), mcp.join(","));
  assert.ok(mcp.includes("client:tools/call live_only_tool"), mcp.join(","));
});

test("a frame with an incomplete leaf earns no seen credit", () => {
  const tools = {
    ...frameRecord("mcpToolCall", "produced", "tools/list", "mcp"),
    raw: { jsonrpc: "2.0", id: 1, result: { tools: [{ name: "a" }] } },
    conformance: { result: "conforms", owner: "mcp", definition: "tools/list result" },
  };
  const unadvertisedCall = {
    ...frameRecord("mcpToolCall", "consumed", "tools/call", "mcp"),
    raw: { jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: "a" } },
    conformance: {
      result: "composite",
      parts: [
        { result: "conforms", owner: "mcp", definition: "tools/call a" },
        { result: "incomplete", owner: "mcp", missing: { missing: "noAdvertisement", tool: "a" } },
      ],
    },
  };

  const audit = buildAudit(
    [recording({ node: "N11", records: [tools, unadvertisedCall] })],
    NO_LIVE,
  );

  assert.ok(audit.unseenKinds["mcp n11"]?.includes("client:tools/call a"));
});

test("an owned-meta defect keeps its leaf owner through the part settlement", () => {
  const record = {
    ...frameRecord("acpServedForRouterSessions", "produced", "response:session/new", "acp"),
    conformance: {
      result: "composite",
      parts: [
        { result: "conforms", owner: "acp", definition: "NewSessionResponse:session/new" },
        {
          result: "violations",
          owner: "routerExtension",
          definition: "SessionResultMeta",
          violations: [
            {
              schemaPath: "SessionResultMeta",
              instancePath: "/result/_meta",
              keyword: "routerCodec",
              detail: "",
            },
          ],
        },
      ],
    },
  };
  const listed = {
    direction: "produced",
    kind: "response:session/new",
    ordinal: 0,
    schemaPath: "SessionResultMeta",
    instancePath: "/result/_meta",
    keyword: "routerCodec",
    detail: "",
    deliverable: "d2",
  };

  const audit = buildAudit(
    [recording({ node: "N9", records: [record], schema: { ...settled(), expected: [listed] } })],
    NO_LIVE,
  );

  const finding = audit.findings.find((candidate) => candidate.kind === "owned-disagreement");
  assert.equal(finding?.owner, "router-extension");
});

test("a client-sent tools/list reply advertises nothing, and the report states its named limit", () => {
  const clientSent = {
    ...frameRecord("mcpToolCall", "consumed", "tools/list", "mcp"),
    raw: { jsonrpc: "2.0", id: 1, result: { tools: [{ name: "client_sent_tool" }] } },
    conformance: { result: "conforms", owner: "mcp", definition: "tools/list result" },
  };

  const audit = buildAudit([recording({ node: "N11", records: [clientSent] })], NO_LIVE);

  assert.ok(!(audit.unseenKinds["mcp n11"] ?? []).includes("client:tools/call client_sent_tool"));
  assert.ok(audit.namedLimits.some((limit) => limit.includes("2^53")));
});
