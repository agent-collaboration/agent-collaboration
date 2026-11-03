import assert from "node:assert/strict";
import { createServer } from "node:http";
import { test } from "node:test";
import { CentralOwner } from "../clients/central-owner.ts";
import { parseFleetSchemaReadback } from "./d1-fleet-schema.ts";

test("D1 project controls use the real declared request shapes and reject one-shot acknowledgement", async (): Promise<void> => {
  const project = "019f0000-0000-7000-8000-000000008101";
  const requests: unknown[] = [];
  let oneShot = false;
  const server = createServer(async (request, response): Promise<void> => {
    let body = "";
    for await (const chunk of request) body += String(chunk);
    const input: unknown = JSON.parse(body);
    requests.push({ url: request.url, method: request.method, input });
    response.setHeader("content-type", "application/json");
    response.end(
      JSON.stringify({
        storage: { kind: oneShot ? "triggered" : requests.length === 1 ? "unavailable" : "idle" },
      }),
    );
  });
  await new Promise<void>((resolve): void => {
    server.listen(0, "127.0.0.1", resolve);
  });
  try {
    const address = server.address();
    if (address === null || typeof address === "string") throw new Error("fixture listener absent");
    const owner = new CentralOwner(`http://127.0.0.1:${address.port}`, {});
    await owner.projectFault(project, true);
    await owner.projectFault(project, false);
    assert.deepEqual(requests, [
      {
        url: "/admin/v1/conformance/runtime",
        method: "POST",
        input: {
          address: { kind: "copy", value: project },
          command: { kind: "projectUnavailable" },
        },
      },
      {
        url: "/admin/v1/conformance/runtime",
        method: "POST",
        input: {
          address: { kind: "copy", value: project },
          command: { kind: "clearProjectFault" },
        },
      },
    ]);
    oneShot = true;
    await assert.rejects(owner.projectFault(project, true));
  } finally {
    await new Promise<void>((resolve, reject): void => {
      server.close((error): void => (error === undefined ? resolve() : reject(error)));
    });
  }
});

test("D1 Fleet schema is full DDL inventory, preserving raw fields and rejecting lease-only or incomplete views", (): void => {
  const raw = [
    {
      kind: "table",
      name: "machine_enrollments",
      definition: "CREATE TABLE machine_enrollments(machine_id TEXT PRIMARY KEY)",
      futureField: "retained",
    },
    {
      kind: "index",
      name: "sqlite_autoindex_machine_enrollments_1",
      table: "machine_enrollments",
      definition: null,
    },
  ];
  const view = parseFleetSchemaReadback(raw);
  assert.deepEqual(view.raw, raw);
  assert.equal(view.objects.length, 2);
  for (const wrong of [[], { peer: "lease-schema-support-only" }, [raw[1]], [raw[0], raw[0]]])
    assert.throws((): unknown => parseFleetSchemaReadback(wrong));
});
