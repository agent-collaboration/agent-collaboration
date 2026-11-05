// Driver wiring through a scripted stand-in MCP server, a real temporary owning store and a peer
// receipt. A narrower control: it proves the case runner sends the right calls in order and
// decides from what comes back, including duplicate dispatch, never a Host or provider path.
import assert from "node:assert/strict";
import { appendFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { tmpdir, userInfo } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { test } from "node:test";
import { z } from "zod";
import { decode, encode } from "../../exact-json.ts";
import { controlPlanSchema, type ControlPlan } from "./control-plan.ts";
import { lifecycleCase } from "./journey-cases.ts";
import { ProviderStoreReadback } from "./role-readback.ts";
import { StreamingMcpClient } from "./streaming-mcp-client.ts";

const SERVICE = "11111111-1111-4111-8111-111111111111";
const TARGET = { endpoint: { serviceId: SERVICE, endpointId: "cursor-local" }, sessionId: "s-1" };
const OWNER = userInfo().username;
const item = (text: string): unknown => ({ itemId: "i-1", kind: { kind: "agentMessage" }, text });
const TURN = [
  { kind: "turnStarted", turnId: "t-1", inputId: "in-1" },
  { kind: "inputAccepted", inputId: "in-1", turnId: "t-1" },
  { kind: "itemStarted", item: item("before") },
  { kind: "itemUpdated", item: item("beforeafter") },
  { kind: "itemCompleted", itemId: "i-1" },
  {
    kind: "turnEnded",
    turnId: "t-1",
    outcome: { kind: "ended", stopReason: "endTurn", localCause: null },
  },
].map((event, index) => ({ sequence: BigInt(index + 3), event }));

const callSchema = z.object({
  id: z.string(),
  params: z.object({ name: z.string(), arguments: z.record(z.string(), z.unknown()) }),
});

interface StandIn {
  readonly store: string;
  readonly receipt: string;
  readonly duplicateDispatch: boolean;
  promptOperation: string | null;
}

function answer(standIn: StandIn, name: string, args: Readonly<Record<string, unknown>>): unknown {
  const observe = (): unknown => ({
    target: TARGET,
    generation: { serviceEpoch: "e", generation: 1 },
    attached: true,
    events:
      args["afterSequence"] === undefined
        ? [{ sequence: 2n, event: { kind: "stateChanged", state: {} } }]
        : args["afterSequence"] === 2n
          ? TURN
          : [],
    endReason: "deadlineReached",
    continuationGap: true,
    epoch: 7n,
  });
  switch (name) {
    case "conversation_create": {
      const database = new DatabaseSync(standIn.store);
      database
        .prepare(
          "INSERT INTO provider_session_records VALUES ('" +
            SERVICE +
            "', 'cursor-local', 's-1', ?, ?)",
        )
        .run(encode({ humanId: OWNER }), encode({ humanId: OWNER }));
      database.close();
      return { kind: "created", operationId: args["operationId"], target: TARGET };
    }
    case "events_observe":
      return observe();
    case "conversation_prompt": {
      standIn.promptOperation = z.string().parse(args["operationId"]);
      const row = `${JSON.stringify({ method: "session/prompt", params: { sessionId: "s-1" } })}\n`;
      appendFileSync(standIn.receipt, standIn.duplicateDispatch ? row + row : row);
      const settlement = {
        target: TARGET,
        stopReason: "endTurn",
        detail: { kind: "providerPrompt", output: { kind: "available", text: "beforeafter" } },
      };
      return {
        kind: "completed",
        target: TARGET,
        operationId: standIn.promptOperation,
        settlement,
      };
    }
    case "conversation_operation_show":
      return {
        operationId: args["operationId"],
        operation: "conversationPrompt",
        target: TARGET,
        stage: "terminal",
        effect: "applied",
        reconciliation: "confirmed",
        terminalStopReason: "end_turn",
      };
    default:
      // conversation_close answers ConversationOperationSubmission; the driver follows it.
      return {
        admission: "admitted",
        operation: {
          operationId: args["operationId"],
          operation: "conversationClose",
          target: TARGET,
          stage: "terminal",
          effect: "applied",
          reconciliation: "confirmed",
        },
      };
  }
}

async function withStandIn(
  duplicateDispatch: boolean,
  body: (
    plan: ControlPlan,
    client: StreamingMcpClient,
    store: ProviderStoreReadback,
  ) => Promise<void>,
): Promise<void> {
  const root = mkdtempSync(join(tmpdir(), "mcp-journey-"));
  const standIn: StandIn = {
    store: join(root, "provider-operations.sqlite"),
    receipt: join(root, "receipt.jsonl"),
    duplicateDispatch,
    promptOperation: null,
  };
  const database = new DatabaseSync(standIn.store);
  database.exec(
    "CREATE TABLE provider_session_records (target_service_id TEXT, target_endpoint_id TEXT, target_session_id TEXT, created_by_json TEXT, approver_json TEXT); CREATE TABLE provider_operations (operation_id TEXT, operation_kind TEXT, target_service_id TEXT, target_endpoint_id TEXT, target_session_id TEXT, stage TEXT, effect TEXT, reconciliation_state TEXT, admitted_at_ms INTEGER, terminal_at_ms INTEGER);",
  );
  database.close();
  writeFileSync(standIn.receipt, "");
  const server: Server = createServer((request, response) => {
    let text = "";
    request.on("data", (chunk: Buffer) => (text += chunk.toString("utf8")));
    request.on("end", () => {
      const call = callSchema.parse(decode(text));
      response.setHeader("content-type", "application/json");
      response.end(
        encode({
          jsonrpc: "2.0",
          id: call.id,
          result: { structuredContent: answer(standIn, call.params.name, call.params.arguments) },
        }),
      );
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (address === null || typeof address === "string") throw new Error("no stand-in port");
  const owner = { kind: "human" as const, humanId: OWNER, source: "ownerLookup" as const };
  const plan = controlPlanSchema.parse({
    repository: root,
    sourceHead: "a".repeat(40),
    buildManifest: root,
    host: { pid: 1, path: root, sha256: "0".repeat(64) },
    router: { path: root, sha256: "0".repeat(64) },
    serviceDirectory: root,
    expectedServiceId: SERVICE,
    expectedServiceEpoch: "22222222-2222-4222-8222-222222222222",
    endpointId: "cursor-local",
    workingDirectory: root,
    actor: owner,
    approver: owner,
    wrongRoleRequester: null,
    peer: { kind: "controlled", receipt: standIn.receipt, releaseSentinel: join(root, "release") },
    cases: ["lifecycle"],
    promptText: "ordinary",
    holdText: "hold",
    effects: { maxSessions: 1, maxPrompts: 1 },
    deadline: "2099-01-01T00:00:00Z",
    evidence: join(root, "evidence"),
    schemaSource: null,
  });
  const store = new ProviderStoreReadback(standIn.store);
  try {
    await body(plan, new StreamingMcpClient(`http://127.0.0.1:${address.port}/mcp`), store);
  } finally {
    store.close();
    server.closeAllConnections();
    await new Promise<void>((resolve) => server.close(() => resolve()));
    rmSync(root, { recursive: true, force: true });
  }
}

test("the lifecycle case qualifies one isolated prompt from the driver's own calls", async () => {
  await withStandIn(false, async (plan, client, store) => {
    const context = {
      client,
      evidence: join(plan.repository, "evidence"),
      deadline: Date.parse(plan.deadline),
    };
    mkdirSync(context.evidence);
    const result = await lifecycleCase({
      context,
      plan,
      store,
      promptText: "ordinary",
      holdText: "hold",
    });
    assert.deepEqual(result.verdict, {
      kind: "qualified",
      turnId: "t-1",
      inputId: "in-1",
      reply: "beforeafter",
    });
  });
});

test("the same driver refuses a duplicate provider dispatch it did not cause", async () => {
  await withStandIn(true, async (plan, client, store) => {
    const context = {
      client,
      evidence: join(plan.repository, "evidence"),
      deadline: Date.parse(plan.deadline),
    };
    mkdirSync(context.evidence);
    const result = await lifecycleCase({
      context,
      plan,
      store,
      promptText: "ordinary",
      holdText: "hold",
    });
    assert.deepEqual(result.verdict, {
      kind: "failed",
      reason: "the controlled peer received 2 prompts, not exactly one",
    });
  });
});
