import assert from "node:assert/strict";
import { readFileSync, readdirSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { once } from "node:events";
import { join } from "node:path";
import { test } from "node:test";
import { z } from "zod";
import { decodeEvidenceJson, evidenceJson } from "../evidence-json.ts";
import { createFixtureDirectory } from "../test-support/fixture-directory.ts";
import { observedObject, readObservedJson, type JsonObject } from "./evidence-models.ts";
import { journeyPlanSchema, operationId } from "./provider-journey-plan.ts";
import { callOnce } from "./provider-journey-rpc.ts";
import {
  closeSession,
  createSession,
  initializeApprover,
  inspectCreatedSession,
  observeSession,
} from "./provider-session-control.ts";
import { errorCategory } from "./runtime-plan.ts";

const directory = new URL("./baselines/", import.meta.url);
const schema = z.object({
  input: z.object({
    name: z.string(),
    action: z.enum(["observe", "inspect", "initialize-approver", "close", "call-once", "create"]),
    plan: z.unknown(),
    manifest: z.unknown(),
    files: z.record(z.string(), z.unknown()),
    responses: z.record(z.string(), z.array(z.unknown())),
  }),
  expected: z.unknown(),
});

function mapStrings(
  value: unknown,
  replace: (text: string, key: string) => string,
  key: string = "",
): unknown {
  if (typeof value === "string") return replace(value, key);
  if (Array.isArray(value)) {
    const result = value.map((item: unknown) => mapStrings(item, replace));
    const operationIndex = result.indexOf("--operation-id");
    if (operationIndex !== -1)
      result[operationIndex + 1] = replace(String(result[operationIndex + 1]), "operationId");
    return result;
  }
  if (value !== null && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value).map(([name, item]) => [name, mapStrings(item, replace, name)]),
    );
  return value;
}

async function invokeControl(input: z.infer<typeof schema>["input"]): Promise<void> {
  const plan = journeyPlanSchema.parse(input.plan);
  switch (input.action) {
    case "observe":
      await observeSession(plan);
      return;
    case "inspect":
      await inspectCreatedSession(plan);
      return;
    case "initialize-approver":
      await initializeApprover(plan);
      return;
    case "close":
      await closeSession(plan);
      return;
    case "create":
      await createSession(plan);
      return;
    case "call-once":
      await callOnce(plan, "test_mutation", {
        operationId: operationId(),
        target: input.files["target-binding"],
      });
      return;
  }
}

for (const filename of readdirSync(directory)
  .filter((name) => /^provider-control-\d+\.json$/.test(name))
  .sort()) {
  const baseline = schema.parse(decodeEvidenceJson(readFileSync(new URL(filename, directory))));
  test(`provider control matches original HTTP exchanges and evidence: ${baseline.input.name}`, async (context): Promise<void> => {
    const root = createFixtureDirectory("provider-control");
    context.after((): void => {
      rmSync(root, { recursive: true, force: true });
    });
    const cli = new URL("../fixtures/provider-create-command.ts", import.meta.url).pathname;
    const calls: JsonObject[] = [];
    const positions = new Map<string, number>();
    const server = createServer(async (request, response): Promise<void> => {
      assert.equal(request.method, "POST");
      assert.equal(request.url, "/mcp");
      assert.equal(request.headers["mcp-protocol-version"], "2025-11-25");
      assert.equal(request.headers.accept, "application/json, text/event-stream");
      const chunks: Buffer[] = [];
      for await (const chunk of request)
        chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(z.string().parse(chunk)));
      const payload = readObservedJson(Buffer.concat(chunks));
      assert.equal(payload.id, 1n);
      assert.equal(payload.method, "tools/call");
      const parameters = z
        .object({ name: z.string(), arguments: z.record(z.string(), z.unknown()) })
        .parse(payload.params);
      calls.push(parameters);
      const position = positions.get(parameters.name) ?? 0;
      positions.set(parameters.name, position + 1);
      const value = baseline.input.responses[parameters.name]?.[position];
      assert.notEqual(value, undefined, `unexpected extra call ${parameters.name}`);
      const body = evidenceJson({
        jsonrpc: "2.0",
        id: 1,
        result: { structuredContent: value, content: [] },
      });
      response.writeHead(200, { "Content-Length": Buffer.byteLength(body) });
      response.end(body);
    });
    context.after(async (): Promise<void> => {
      server.close();
      await once(server, "close");
    });
    server.listen(0, "127.0.0.1");
    await once(server, "listening");
    const address = server.address();
    if (address === null || typeof address === "string") throw new Error("missing owned listener");
    const url = `http://127.0.0.1:${address.port}/mcp`;
    const input = schema.shape.input.parse(
      mapStrings(baseline.input, (text) =>
        text.replaceAll("<ROOT>", root).replaceAll("<CLI>", cli).replaceAll("<RPC>", url),
      ),
    );
    for (const name of ["evidence", "service", "workspace"]) mkdirSync(join(root, name));
    writeFileSync(join(root, "service", "service.json"), evidenceJson(input.manifest));
    for (const [name, value] of Object.entries(input.files))
      writeFileSync(join(root, "evidence", `${name}.json`), evidenceJson(value));
    const plan = journeyPlanSchema.parse(input.plan);
    const stdout: unknown[] = [];
    context.mock.method(console, "log", (text: string): void => {
      stdout.push(decodeEvidenceJson(text));
    });
    let failure: { readonly category: string; readonly message: string } | null = null;
    try {
      await invokeControl(input);
    } catch (error) {
      failure = {
        category: errorCategory(error),
        message: error instanceof Error ? error.message : String(error),
      };
    }
    const files = Object.fromEntries(
      readdirSync(plan.evidence)
        .filter((name) => name.endsWith(".json"))
        .sort()
        .map((name) => [name, readObservedJson(readFileSync(join(plan.evidence, name)))]),
    );
    const actual = mapStrings({ calls, files, stdout, failure }, (text, key) =>
      key === "operationId"
        ? "<OPERATION>"
        : key === "at"
          ? "<TIMESTAMP>"
          : text.replaceAll(root, "<ROOT>").replaceAll(cli, "<CLI>").replaceAll(url, "<RPC>"),
    );
    assert.deepEqual(actual, baseline.expected);
  });
}

test("provider operation IDs retain UUIDv7 timestamp and variant", (): void => {
  const before = Date.now();
  const value = operationId();
  assert.match(value, /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  const timestamp = Number.parseInt(value.replaceAll("-", "").slice(0, 12), 16);
  assert.ok(before <= timestamp && timestamp <= Date.now());
  assert.notEqual(value, operationId());
  assert.equal(observedObject({ operationId: value }).operationId, value);
});
