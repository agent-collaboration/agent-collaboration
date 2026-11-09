// Driver wiring through a scripted stand-in MCP server, a real temporary owning store and a peer
// receipt. A narrower control: it proves the case runner sends the right calls in order and
// decides from what comes back, including duplicate dispatch, never a Host or provider path.
import { listenOnTestPort } from "../test-support/loopback-test-port.ts";
import assert from "node:assert/strict";
import { appendFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { tmpdir, userInfo } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { test } from "node:test";
import { z } from "zod";
import { decode, encode } from "../../../exact-json.ts";
import { controlPlanSchema, type ControlPlan } from "../admission/control-plan.ts";
import { lifecycleCase } from "./journey-cases.ts";
import { runCases } from "../run.ts";
import { ProviderStoreReadback } from "../evidence/role-readback.ts";
import { StreamingMcpClient } from "../transport/streaming-mcp-client.ts";

const SERVICE = "11111111-1111-4111-8111-111111111111";
const TARGET = { endpoint: { serviceId: SERVICE, endpointId: "cursor-local" }, sessionId: "s-1" };
const OWNER = userInfo().username;
const item = (text: string): unknown => ({ itemId: "i-1", kind: { kind: "agentMessage" }, text });
// A direct prompt in the producer's shape: no inputAccepted (steer only) and snake_case
// TurnOutcome fields (session_event.rs:141-151), as the first controlled Host run recorded.
function turn(outcome: unknown): { sequence: bigint; event: unknown }[] {
  return [
    { kind: "turnStarted", turnId: "t-1", inputId: "in-1" },
    {
      kind: "itemStarted",
      item: { itemId: "u-1", kind: { kind: "userMessage" }, text: "ordinary" },
    },
    { kind: "itemStarted", item: item("before") },
    { kind: "itemUpdated", item: item("beforeafter") },
    { kind: "itemCompleted", itemId: "i-1" },
    { kind: "turnEnded", turnId: "t-1", outcome },
  ].map((event, index) => ({ sequence: BigInt(index + 3), event }));
}
const TURN = turn({ kind: "ended", stop_reason: "endTurn", local_cause: null });
// The camelCase outcome the driver once invented; the real Host never sends it.
const INVENTED_TURN = turn({ kind: "ended", stopReason: "endTurn", localCause: null });

const callSchema = z.object({
  id: z.string(),
  params: z.object({ name: z.string(), arguments: z.record(z.string(), z.unknown()) }),
});

/** How the stand-in answers conversation_create: faithfully, or self-consistently wrong. */
type CreateAnswer = "faithful" | "otherEndpoint" | "otherService" | "foreignOperation";
type CreateSettings = "complete" | "notApplied" | "createdWithoutSettings";

interface StandIn {
  readonly store: string;
  readonly receipt: string;
  readonly duplicateDispatch: boolean;
  readonly createAnswer: CreateAnswer;
  readonly createSettings: CreateSettings;
  /** Answers the post-prompt observation with the invented outcome, failing after create. */
  readonly inventedTurnEnd: boolean;
  /** Answers create faithfully but leaves no owning-store record, failing the role readback. */
  readonly omitStoreRecord: boolean;
  readonly calls: string[];
  promptOperation: string | null;
}

function createdTarget(answer: CreateAnswer): typeof TARGET {
  if (answer === "otherEndpoint")
    return { ...TARGET, endpoint: { ...TARGET.endpoint, endpointId: "claude-local" } };
  if (answer === "otherService")
    return {
      ...TARGET,
      endpoint: { ...TARGET.endpoint, serviceId: "99999999-9999-4999-8999-999999999999" },
    };
  return TARGET;
}

function createSettingsFields(settings: CreateSettings): Readonly<Record<string, unknown>> {
  return {
    kind: settings === "createdWithoutSettings" ? "createdWithoutSettings" : "created",
    ...(settings === "createdWithoutSettings" ? { applied: [], failed: [] } : {}),
    ...(settings === "notApplied" ? { notApplied: [{ setting: "model", value: "haiku" }] } : {}),
  };
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
          ? standIn.inventedTurnEnd
            ? INVENTED_TURN
            : TURN
          : [],
    endReason: "deadlineReached",
    continuationGap: true,
    epoch: 7n,
  });
  switch (name) {
    case "conversation_create": {
      // The store record matches whatever target is answered, so a wrong answer is fully
      // self-consistent and only a check against the plan can catch it.
      const target = createdTarget(standIn.createAnswer);
      const database = new DatabaseSync(standIn.store);
      if (!standIn.omitStoreRecord)
        database
          .prepare("INSERT INTO provider_session_records VALUES (?, ?, ?, ?, ?)")
          .run(
            target.endpoint.serviceId,
            target.endpoint.endpointId,
            target.sessionId,
            encode({ humanId: OWNER }),
            encode({ humanId: OWNER }),
          );
      database.close();
      const operationId =
        standIn.createAnswer === "foreignOperation"
          ? "0190a000-0000-7000-8000-00000000f0f0"
          : args["operationId"];
      return { ...createSettingsFields(standIn.createSettings), operationId, target };
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
        operation:
          args["operationId"] === standIn.promptOperation
            ? "conversationPrompt"
            : "conversationClose",
        target: TARGET,
        stage: "terminal",
        effect: "applied",
        reconciliation: "confirmed",
        terminalStopReason: "end_turn",
      };
    case "conversation_operation_wait":
      return {
        operation: {
          operationId: args["operationId"],
          operation: "conversationClose",
          target: TARGET,
          stage: "terminal",
          effect: "applied",
          reconciliation: "confirmed",
        },
        output: { kind: "available", settlement: { kind: "closed", target: TARGET } },
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
  options: {
    readonly duplicateDispatch?: boolean;
    readonly createAnswer?: CreateAnswer;
    readonly createSettings?: CreateSettings;
    readonly inventedTurnEnd?: boolean;
    readonly omitStoreRecord?: boolean;
  },
  body: (
    plan: ControlPlan,
    client: StreamingMcpClient,
    store: ProviderStoreReadback,
    standIn: StandIn,
  ) => Promise<void>,
): Promise<void> {
  const root = mkdtempSync(join(tmpdir(), "mcp-journey-"));
  const standIn: StandIn = {
    store: join(root, "provider-operations.sqlite"),
    receipt: join(root, "receipt.jsonl"),
    duplicateDispatch: options.duplicateDispatch ?? false,
    createAnswer: options.createAnswer ?? "faithful",
    createSettings: options.createSettings ?? "complete",
    inventedTurnEnd: options.inventedTurnEnd ?? false,
    omitStoreRecord: options.omitStoreRecord ?? false,
    calls: [],
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
      standIn.calls.push(call.params.name);
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
  const port = await listenOnTestPort(server);
  const owner = { kind: "human" as const, humanId: OWNER, source: "ownerLookup" as const };
  // Built inside the teardown's reach: a plan that fails to parse must fail the test, not leave
  // the stand-in server listening.
  let store: ProviderStoreReadback | null = null;
  try {
    const plan = controlPlanSchema.parse({
      repository: root,
      sourceHead: "a".repeat(40),
      buildManifest: root,
      host: { kind: "automationDebugHost", pid: 1, path: root, sha256: "0".repeat(64) },
      hostContext: null,
      router: { path: root, sha256: "0".repeat(64) },
      serviceDirectory: root,
      expectedServiceId: SERVICE,
      expectedServiceEpoch: "22222222-2222-4222-8222-222222222222",
      endpointId: "cursor-local",
      workingDirectory: root,
      actor: owner,
      approver: owner,
      wrongRoleRequester: null,
      peer: {
        kind: "controlled",
        receipt: standIn.receipt,
        releaseSentinel: join(root, "release"),
      },
      cases: ["lifecycle"],
      promptText: "ordinary",
      holdText: "hold",
      effects: { maxSessions: 1, maxPrompts: 1 },
      deadline: "2099-01-01T00:00:00Z",
      evidence: join(root, "evidence"),
      schemaSource: null,
      continuation: null,
    });
    store = new ProviderStoreReadback(standIn.store);
    await body(plan, new StreamingMcpClient(`http://127.0.0.1:${port}/mcp`), store, standIn);
  } finally {
    store?.close();
    server.closeAllConnections();
    await new Promise<void>((resolve) => server.close(() => resolve()));
    rmSync(root, { recursive: true, force: true });
  }
}

test("the lifecycle case qualifies one isolated prompt from the driver's own calls", async () => {
  await withStandIn({}, async (plan, client, store) => {
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
      knownTargets: [],
      promptText: "ordinary",
      holdText: "hold",
    });
    assert.deepEqual(result.verdict, {
      kind: "qualified",
      turnId: "t-1",
      inputId: "in-1",
      reply: "beforeafter",
      dispatchWitness: "controlledReceipt",
    });
  });
});

test("the same driver refuses a duplicate provider dispatch it did not cause", async () => {
  await withStandIn({ duplicateDispatch: true }, async (plan, client, store) => {
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
      knownTargets: [],
      promptText: "ordinary",
      holdText: "hold",
    });
    assert.deepEqual(result.verdict, {
      kind: "failed",
      reason: "the controlled peer received 2 prompts, not exactly one",
    });
  });
});

const MUTATING_TOOLS = new Set([
  "conversation_prompt",
  "conversation_close",
  "conversation_cancel",
  "conversation_resume",
  "conversation_load",
  "conversation_settings_accept",
  "conversation_settings_set",
]);

test("a self-consistent create answer for another endpoint, service or operation stops all later mutation", async () => {
  for (const createAnswer of ["otherEndpoint", "otherService", "foreignOperation"] as const) {
    await withStandIn({ createAnswer }, async (plan, client, store, standIn) => {
      const context = {
        client,
        evidence: join(plan.repository, "evidence"),
        deadline: Date.parse(plan.deadline),
      };
      mkdirSync(context.evidence);
      await assert.rejects(
        lifecycleCase({
          context,
          plan,
          store,
          knownTargets: [],
          promptText: "ordinary",
          holdText: "hold",
        }),
        /create/,
        createAnswer,
      );
      const afterCreate = standIn.calls.slice(standIn.calls.indexOf("conversation_create") + 1);
      assert.deepEqual(
        afterCreate.filter((tool) => MUTATING_TOOLS.has(tool)),
        [],
        createAnswer,
      );
    });
  }
});

test("a failure after a validated create keeps that Session as the case's known target", async () => {
  await withStandIn({ inventedTurnEnd: true }, async (plan, client, store, standIn) => {
    const context = {
      client,
      evidence: join(plan.repository, "evidence"),
      deadline: Date.parse(plan.deadline),
    };
    mkdirSync(context.evidence);
    const [result, ...rest] = await runCases(plan, context, store);
    assert.deepEqual(rest, []);
    assert.equal(result?.verdict.kind, "failed");
    assert.deepEqual(result?.target, TARGET);
    // The failing observation is the last input: no retry, close or other later mutation.
    const afterPrompt = standIn.calls.slice(standIn.calls.indexOf("conversation_prompt") + 1);
    assert.deepEqual(
      afterPrompt.filter((tool) => MUTATING_TOOLS.has(tool)),
      [],
    );
  });
});

test("a self-consistently wrong create answer is never kept as a known target", async () => {
  for (const createSettings of ["complete", "notApplied", "createdWithoutSettings"] as const) {
    for (const createAnswer of ["otherEndpoint", "otherService", "foreignOperation"] as const) {
      await withStandIn({ createAnswer, createSettings }, async (plan, client, store, standIn) => {
        const context = {
          client,
          evidence: join(plan.repository, "evidence"),
          deadline: Date.parse(plan.deadline),
        };
        mkdirSync(context.evidence);
        const [result] = await runCases(plan, context, store);
        assert.equal(result?.verdict.kind, "failed", createAnswer);
        assert.equal(result?.target, null, createAnswer);
        assert.match(
          result?.verdict.kind === "failed" ? result.verdict.reason : "",
          createAnswer === "foreignOperation"
            ? /create answer names operation/
            : /another service or endpoint/,
        );
        const afterCreate = standIn.calls.slice(standIn.calls.indexOf("conversation_create") + 1);
        assert.deepEqual(
          afterCreate.filter((tool) => MUTATING_TOOLS.has(tool)),
          [],
          createAnswer,
        );
      });
    }
  }
});

test("a store failure after a validated create still names that Session, with no later input", async () => {
  await withStandIn({ omitStoreRecord: true }, async (plan, client, store, standIn) => {
    const context = {
      client,
      evidence: join(plan.repository, "evidence"),
      deadline: Date.parse(plan.deadline),
    };
    mkdirSync(context.evidence);
    const [result] = await runCases(plan, context, store);
    assert.equal(result?.verdict.kind, "failed");
    assert.match(result?.verdict.kind === "failed" ? result.verdict.reason : "", /owning store/);
    assert.deepEqual(result?.target, TARGET);
    const afterCreate = standIn.calls.slice(standIn.calls.indexOf("conversation_create") + 1);
    assert.deepEqual(
      afterCreate.filter((tool) => MUTATING_TOOLS.has(tool)),
      [],
    );
  });
});
