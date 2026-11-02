import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { once } from "node:events";
import { mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type Server, type ServerResponse } from "node:http";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { z } from "zod";
import { decodeEvidenceJson, evidenceJson } from "../evidence-json.ts";
import { arrayField } from "../exact-json.ts";
import { createFixtureDirectory } from "../test-support/fixture-directory.ts";
import { observedObject, readObservedJson, type JsonObject } from "./evidence-models.ts";
import { journeyPlanSchema, type JourneyPlan } from "./provider-journey-plan.ts";

export interface JourneyRun {
  readonly stdout: readonly unknown[];
  readonly exit: number;
}
interface CallbackState {
  kind: "question" | "approval" | "complete";
}
interface FixtureModels {
  readonly root: string;
  readonly service: string;
  readonly evidence: string;
  readonly workspace: string;
  readonly marker: string;
  readonly command: string;
  readonly serviceId: string;
  readonly target: JsonObject;
  readonly approver: JsonObject;
  readonly capture: JsonObject;
  readonly operation: JsonObject;
  readonly record: JsonObject;
  readonly question: JsonObject;
  readonly approval: JsonObject;
}
interface FixtureServer {
  readonly server: Server;
  readonly state: CallbackState;
  readonly mutations: JsonObject[];
  readonly waiting: { readonly name: string; readonly response: ServerResponse }[];
}

function normalize(value: unknown, root: string, key: string = ""): unknown {
  if (typeof value === "string")
    return key === "operationId"
      ? "<OPERATION>"
      : key === "at"
        ? "<TIMESTAMP>"
        : value.replaceAll(root, "<ROOT>");
  if (Array.isArray(value)) return value.map((item: unknown) => normalize(item, root));
  if (value !== null && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value).map(([name, item]) => [name, normalize(item, root, name)]),
    );
  return value;
}

function reply(response: ServerResponse, value: unknown): void {
  const body = evidenceJson({
    jsonrpc: "2.0",
    id: 1,
    result: { structuredContent: value, content: [] },
  });
  response.writeHead(200, {
    "Content-Type": "application/json",
    "Content-Length": Buffer.byteLength(body),
  });
  response.end(body);
}

function createModels(root: string): FixtureModels {
  const service = join(root, "service");
  const evidence = join(root, "evidence");
  const workspace = join(root, "workspace");
  for (const path of [service, evidence, workspace]) mkdirSync(path, { mode: 0o700 });
  const baseline = readObservedJson(
    readFileSync(new URL("./baselines/callback-08.json", import.meta.url)),
  );
  const args = arrayField(baseline, "args");
  const capture = observedObject(args[0]);
  const operation = observedObject(args[1]);
  const serviceId = "00000000-0000-7000-8000-000000000001";
  const target = { endpoint: { serviceId, endpointId: "claude-local" }, sessionId: "requester" };
  const approver = { ...target, sessionId: "approver" };
  const actor = { kind: "session", session: approver };
  capture.target = target;
  capture.observation = { ...observedObject(capture.observation), target };
  operation.target = target;
  Object.assign(operation, { stage: "terminal", effect: "applied", reconciliation: "confirmed" });
  const catalog = observedObject(
    arrayField(
      readObservedJson(readFileSync(new URL("./baselines/callback-00.json", import.meta.url))),
      "args",
    )[0],
  ).settingsCatalog;
  const record = {
    target,
    state: "idle",
    capabilities: { elicitation: true },
    settingsCatalog: catalog,
  };
  const question = {
    requestId: "question",
    requester: target,
    approver: actor,
    fields: [
      {
        kind: "singleChoice",
        fieldId: "color",
        required: true,
        options: [
          { optionId: "blue", label: "Blue" },
          { optionId: "green", label: "Green" },
        ],
      },
    ],
  };
  const approval = {
    requestId: "approval",
    requester: target,
    approver: actor,
    options: [{ effect: "allow", scope: "once", optionId: "allow-once" }],
  };
  const marker = join(workspace, "automated-approval-marker");
  const command = `printf '%s\\n' 'VERIFICATION_MARKER' > ${marker}`;
  return {
    root,
    service,
    evidence,
    workspace,
    marker,
    command,
    serviceId,
    target,
    approver,
    capture,
    operation,
    record,
    question,
    approval,
  };
}

function createDatabase(model: FixtureModels): DatabaseSync {
  const connection = new DatabaseSync(join(model.service, "interaction.sqlite"));
  connection.exec(
    "CREATE TABLE interaction_history_revision(metadata_id INTEGER PRIMARY KEY,revision INTEGER NOT NULL); INSERT INTO interaction_history_revision VALUES(1,0); CREATE TABLE typed_interaction_history(request_id TEXT PRIMARY KEY,record_json TEXT NOT NULL,created_at TEXT NOT NULL)",
  );
  const insert = connection.prepare("INSERT INTO typed_interaction_history VALUES(?,?,?)");
  insert.run(
    "question",
    evidenceJson({
      kind: "question",
      request: { requestId: "question" },
      state: { kind: "pending" },
      rawExtension: { retained: true },
    }),
    "2026-10-26T01:02:03.123456789Z",
  );
  insert.run(
    "approval",
    evidenceJson({
      kind: "approval",
      request: {
        requestId: "approval",
        description: `owned marker\ncommand: ${model.command}\n`,
        subject: { toolCall: { toolCallId: "approval-tool", kind: "Execute" } },
      },
      state: { kind: "pending" },
    }),
    "2026-10-26T01:02:03.223456789Z",
  );
  return connection;
}

function createFixtureServer(model: FixtureModels, connection: DatabaseSync): FixtureServer {
  const state: CallbackState = { kind: "question" };
  const mutations: JsonObject[] = [];
  const waiting: { readonly name: string; readonly response: ServerResponse }[] = [];
  const settle = (requestId: string, settled: unknown): void => {
    const row = z
      .object({ record_json: z.string() })
      .parse(
        connection
          .prepare("SELECT record_json FROM typed_interaction_history WHERE request_id=?")
          .get(requestId),
      );
    const value = readObservedJson(row.record_json);
    value.state = settled;
    connection
      .prepare("UPDATE typed_interaction_history SET record_json=? WHERE request_id=?")
      .run(evidenceJson(value), requestId);
    connection.exec("UPDATE interaction_history_revision SET revision=revision+1");
  };
  const server = createServer(async (request, response): Promise<void> => {
    const chunks: Buffer[] = [];
    for await (const chunk of request)
      chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(z.string().parse(chunk)));
    const payload = readObservedJson(Buffer.concat(chunks));
    const parameters = z
      .object({ name: z.string(), arguments: z.record(z.string(), z.unknown()) })
      .parse(payload.params);
    switch (parameters.name) {
      case "provider_session_inspect":
        reply(response, { record: model.record });
        return;
      case "events_observe":
      case "conversation_prompt":
        waiting.push({ name: parameters.name, response });
        if (parameters.name === "conversation_prompt") mutations.push(parameters);
        return;
      case "question_list":
        reply(response, { questions: state.kind === "question" ? [model.question] : [] });
        return;
      case "approval_list":
        reply(response, { approvals: state.kind === "approval" ? [model.approval] : [] });
        return;
      case "question_answer":
        assert.equal(state.kind, "question");
        assert.equal(parameters.arguments.requestId, "question");
        mutations.push(parameters);
        settle("question", {
          kind: "answered",
          content: observedObject(parameters.arguments.response).content,
        });
        state.kind = "approval";
        reply(response, { answered: true });
        return;
      case "approval_decide":
        assert.equal(state.kind, "approval");
        assert.equal(parameters.arguments.optionId, "allow-once");
        mutations.push(parameters);
        settle("approval", { kind: "decided", option_id: parameters.arguments.optionId });
        state.kind = "complete";
        writeFileSync(model.marker, "VERIFICATION_MARKER\n", { flag: "wx" });
        reply(response, { decided: true });
        for (const pending of waiting)
          reply(
            pending.response,
            pending.name === "events_observe"
              ? { ...observedObject(model.capture.observation), events: model.capture.events }
              : { settled: true },
          );
        return;
      case "conversation_operation_show":
        reply(response, model.operation);
        return;
      default:
        response.writeHead(500);
        response.end("unexpected call");
        throw new Error(`unexpected ${parameters.name}`);
    }
  });
  return { server, state, mutations, waiting };
}

function preparePlan(model: FixtureModels, url: string): JourneyPlan {
  const { service, evidence, root, target, approver, serviceId } = model;
  writeFileSync(
    join(service, "service.json"),
    evidenceJson({
      serviceId,
      serviceEpoch: "epoch",
      serviceVersion: "test",
      machineLabel: "synthetic",
      routerProxyEndpoint: "http://127.0.0.1:1",
      mcp: { url },
    }),
  );
  writeFileSync(join(evidence, "target-binding.json"), evidenceJson(target));
  const provenance = join(root, "tool-provenance.txt");
  writeFileSync(provenance, "synthetic fixture tool catalog\n");
  return journeyPlanSchema.parse({
    cli: process.execPath,
    evidence,
    service_directory: service,
    caller_session_id: "caller",
    endpoint_id: "claude-local",
    settings: { mode: "manual-mode", model: "provider-model", effort: "low" },
    tools: {
      question_tool: "fixture_question",
      execution_tool: "fixture_execute",
      question_method: "elicitation/create",
      evidence_path: provenance,
      evidence_sha256: createHash("sha256").update(readFileSync(provenance)).digest("hex"),
    },
    approver,
  });
}

export async function runCallbackFixture(
  driver: (plan: JourneyPlan, planPath: string) => Promise<JourneyRun>,
): Promise<unknown> {
  const root = createFixtureDirectory("callback-journey");
  const model = createModels(root);
  const connection = createDatabase(model);
  const { server, state, mutations, waiting } = createFixtureServer(model, connection);
  try {
    server.listen(0, "127.0.0.1");
    await once(server, "listening");
    const address = server.address();
    if (address === null || typeof address === "string") throw new Error("missing owned listener");
    const plan = preparePlan(model, `http://127.0.0.1:${address.port}/mcp`);
    const planPath = join(root, "plan.json");
    writeFileSync(planPath, evidenceJson(plan));
    const result = await driver(plan, planPath);
    assert.equal(result.exit, 0);
    assert.equal(state.kind, "complete");
    assert.equal(mutations.length, 3);
    assert.equal(waiting.length, 2);
    const files = Object.fromEntries(
      readdirSync(model.evidence)
        .filter((name) => name.endsWith(".json"))
        .sort()
        .map((name) => [name, decodeEvidenceJson(readFileSync(join(model.evidence, name)))]),
    );
    return normalize(
      { mutations, files, stdout: result.stdout, marker: readFileSync(model.marker, "utf8") },
      root,
    );
  } finally {
    server.closeAllConnections();
    server.close();
    if (server.listening) await once(server, "close");
    connection.close();
    rmSync(root, { recursive: true, force: true });
  }
}
