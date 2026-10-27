import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { once } from "node:events";
import { copyFileSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { join } from "node:path";
import { z } from "zod";
import { decodeEvidenceJson, evidenceJson } from "../../evidence-json.ts";
import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";
import { observedObject, readObservedJson, type JsonObject } from "../evidence-models.ts";
import { boardPlanSchema } from "../tailnet-plans.ts";

function replaceRoot(value: unknown, root: string): unknown {
  if (typeof value === "string") return value.replaceAll(root, "<ROOT>");
  if (Array.isArray(value)) return value.map((item: unknown) => replaceRoot(item, root));
  if (value !== null && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value).map(([name, item]) => [name, replaceRoot(item, root)]),
    );
  return value;
}
export async function runReadbackFixture(
  driver: (planPath: string) => Promise<void>,
): Promise<unknown> {
  const root = createFixtureDirectory("readback");
  const destination = join(root, "d");
  const evidence = join(root, "e");
  const service = join(destination, "agent-communication");
  for (const path of [destination, evidence, service]) mkdirSync(path, { mode: 0o700 });
  const baseline = readObservedJson(
    readFileSync(new URL("baselines/tailnet-peer-success.bin", import.meta.url)),
  );
  const plan = boardPlanSchema.parse({
    ...observedObject(baseline.plan),
    destination_root: destination,
    evidence,
  });
  const database = join(service, "project-board.sqlite");
  const storedOracle = readObservedJson(
    readFileSync(new URL("baselines/tailnet-oracle-21.json", import.meta.url)),
  );
  copyFileSync(
    new URL(`baselines/${z.string().parse(storedOracle.databaseFile)}`, import.meta.url),
    database,
  );
  const before = createHash("sha256").update(readFileSync(database)).digest("hex");
  writeFileSync(
    join(service, "service.json"),
    evidenceJson({
      serviceId: plan.destination_service_id,
      serviceEpoch: plan.destination_service_epoch,
    }),
  );
  const calls = z
    .array(z.looseObject({ reply: z.object({ bodyBase64: z.string() }) }))
    .parse(baseline.calls);
  const first = calls[0];
  if (first === undefined) throw new Error("original inventory missing");
  const inventory = observedObject(
    observedObject(readObservedJson(Buffer.from(first.reply.bodyBase64, "base64")).result)
      .structuredContent,
  );
  const expected = observedObject(baseline.expected);
  const originalFiles = observedObject(expected.files);
  const record = observedObject(originalFiles["peer-result.json"]).readback;
  const requests: JsonObject[] = [];
  const server = createServer(async (request, response): Promise<void> => {
    assert.equal(request.method, "POST");
    assert.equal(request.url, "/mcp");
    const chunks: Buffer[] = [];
    for await (const chunk of request)
      chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(z.string().parse(chunk)));
    const payload = readObservedJson(Buffer.concat(chunks));
    const parameters = z
      .object({
        name: z.enum(["endpoints_list", "board_message_show"]),
        arguments: z.record(z.string(), z.unknown()),
      })
      .parse(payload.params);
    requests.push(parameters);
    if (parameters.name === "board_message_show")
      assert.equal(parameters.arguments.messageId, plan.marker.message_id);
    response.writeHead(200, { "Content-Type": "application/json" });
    response.end(
      evidenceJson({
        jsonrpc: "2.0",
        id: 1n,
        result: { structuredContent: parameters.name === "endpoints_list" ? inventory : record },
      }),
    );
  });
  try {
    server.listen(join(service, "control.sock"));
    await once(server, "listening");
    const planPath = join(root, "plan.json");
    writeFileSync(planPath, evidenceJson(plan));
    await driver(planPath);
    assert.equal(createHash("sha256").update(readFileSync(database)).digest("hex"), before);
    const files = Object.fromEntries(
      readdirSync(evidence)
        .sort()
        .map((name) => [name, decodeEvidenceJson(readFileSync(join(evidence, name)))]),
    );
    return replaceRoot({ requests, files }, root);
  } finally {
    server.closeAllConnections();
    server.close();
    if (server.listening) await once(server, "close");
    rmSync(root, { recursive: true, force: true });
  }
}
