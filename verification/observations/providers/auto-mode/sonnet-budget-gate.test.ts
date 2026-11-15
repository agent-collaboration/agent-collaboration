import assert from "node:assert/strict";
import { createServer } from "node:http";
import { test } from "node:test";
import { listenOnTestPort } from "../../mcp-control/test-support/loopback-test-port.ts";
import {
  SonnetRequestBudget,
  startSonnetBudgetGate,
  type BudgetEvent,
  preInferenceRefusal,
} from "./sonnet-budget-gate.ts";

test("fourth Sonnet request is refused before any upstream request or byte, including concurrent arrivals", async (): Promise<void> => {
  const events: BudgetEvent[] = [];
  let requests = 0;
  let bytes = 0;
  const upstream = createServer((request, response): void => {
    requests++;
    assert.ok(events.some((e) => e.kind === "reserved"));
    request.on("data", (chunk: Buffer): void => {
      bytes += chunk.length;
    });
    request.on("end", (): void => {
      response.writeHead(200, { "content-type": "text/event-stream" });
      response.write("data: first\n\n");
      response.end("data: last\n\n");
    });
  });
  const routerPort = await listenOnTestPort(upstream);
  const budget = new SonnetRequestBudget(0, (event) => events.push(event));
  const gate = await startSonnetBudgetGate({ routerPort, budget });
  try {
    const payload = JSON.stringify({
      model: "claude-sonnet-test",
      messages: [{ content: "PRIVATE_PAYLOAD" }],
    });
    const results = await Promise.all(
      Array.from({ length: 4 }, async (): Promise<number> => {
        const r = await fetch(gate.baseUrl + "/v1/messages", {
          method: "POST",
          headers: { authorization: "PRIVATE_TOKEN" },
          body: payload,
        });
        if (r.status === 200) assert.equal(await r.text(), "data: first\n\ndata: last\n\n");
        else await r.text();
        return r.status;
      }),
    );
    assert.deepEqual(results.sort(), [200, 200, 200, 429]);
    assert.equal(requests, 3);
    assert.equal(bytes, Buffer.byteLength(payload) * 3);
    assert.equal(events.filter((e) => e.kind === "refused").length, 1);
    assert.deepEqual(budget.summary(), {
      sonnetUsed: 3,
      sonnetBillable: 3,
      sonnetPending: 0,
      byModel: { "claude-sonnet-test": 3 },
      refusedByModel: {},
      blocked: 1,
    });
    assert.equal(JSON.stringify(events).includes("PRIVATE_PAYLOAD"), false);
    assert.equal(JSON.stringify(events).includes("PRIVATE_TOKEN"), false);
  } finally {
    await gate.close();
    upstream.closeAllConnections();
    await new Promise<void>((resolve) => upstream.close(() => resolve()));
  }
});

test("restored total cap cannot be reset by a new gate and classifier models remain separately counted", (): void => {
  const events: BudgetEvent[] = [];
  const budget = new SonnetRequestBudget(2, (event) => events.push(event));
  const classifier = budget.reserve("claude-haiku-classifier");
  const sonnet = budget.reserve("claude-sonnet-test");
  assert.ok(classifier);
  assert.ok(sonnet);
  assert.equal(budget.reserve("claude-sonnet-other"), null);
  budget.settle(classifier, null);
  budget.settle(sonnet, null);
  assert.deepEqual(budget.summary(), {
    sonnetUsed: 3,
    sonnetBillable: 3,
    sonnetPending: 0,
    byModel: { "claude-haiku-classifier": 1, "claude-sonnet-test": 1 },
    refusedByModel: {},
    blocked: 1,
  });
  assert.equal(events.filter((e) => e.kind === "refused").length, 1);
  assert.throws(() => new SonnetRequestBudget(4, () => {}), /invalid/);
});

test("explicit pre-inference refusals release slots; three billable responses still block the fourth", async (): Promise<void> => {
  const events: BudgetEvent[] = [];
  let requests = 0;
  const errors = [
    { status: 404, type: "model_not_found" },
    { status: 401, type: "authentication_error" },
    { status: 503, type: "selection_rejected" },
  ];
  const upstream = createServer((request, response): void => {
    const index = requests++;
    request.resume();
    request.on("end", (): void => {
      const error = errors[index];
      response.writeHead(error?.status ?? 200, { "content-type": "application/json" });
      response.end(
        error
          ? JSON.stringify({ error: { type: error.type, message: "PRIVATE_RESPONSE" } })
          : '{"ok":true}',
      );
    });
  });
  const routerPort = await listenOnTestPort(upstream);
  const budget = new SonnetRequestBudget(0, (e) => events.push(e));
  const gate = await startSonnetBudgetGate({ routerPort, budget });
  try {
    for (let index = 0; index < 6; index++) {
      const response = await fetch(gate.baseUrl + "/v1/messages", {
        method: "POST",
        body: '{"model":"claude-sonnet-test"}',
      });
      assert.equal(response.status, errors[index]?.status ?? 200);
      await response.text();
    }
    assert.equal(budget.summary().sonnetBillable, 3);
    assert.deepEqual(budget.summary().refusedByModel, { "claude-sonnet-test": 3 });
    const denied = await fetch(gate.baseUrl + "/v1/messages", {
      method: "POST",
      body: '{"model":"claude-sonnet-test"}',
    });
    assert.equal(denied.status, 429);
    await denied.text();
    assert.equal(requests, 6);
    assert.equal(JSON.stringify(events).includes("PRIVATE_RESPONSE"), false);
  } finally {
    await gate.close();
    upstream.closeAllConnections();
    await new Promise<void>((resolve) => upstream.close(() => resolve()));
  }
});

test("unknown outcomes and errors after response starts stay conservatively charged", (): void => {
  for (const [status, body] of [
    [200, '{"error":{"type":"model_not_found"}}'],
    [404, '{"error":{"type":"not_found_error"}}'],
    [500, '{"error":{"type":"api_error"}}'],
    [401, "malformed"],
  ] satisfies readonly [number, string][])
    assert.equal(preInferenceRefusal(status, Buffer.from(body)), null);
  const budget = new SonnetRequestBudget(0, () => {});
  const id = budget.reserve("claude-sonnet-test");
  assert.ok(id);
  budget.settle(id, null);
  assert.equal(budget.summary().sonnetBillable, 1);
  assert.throws(() => budget.settle(id, "auth"), /settled/);
});

test("in-flight reservations hold the cap until the explicit refusal settles", (): void => {
  const budget = new SonnetRequestBudget(0, () => {});
  const ids = Array.from({ length: 3 }, () => budget.reserve("claude-sonnet-test"));
  assert.equal(budget.summary().sonnetPending, 3);
  assert.equal(budget.reserve("claude-sonnet-test"), null);
  const first = ids[0];
  assert.ok(first);
  budget.settle(first, "model_not_found");
  assert.ok(budget.reserve("claude-sonnet-test"));
  assert.equal(budget.summary().sonnetPending, 3);
  assert.equal(budget.summary().sonnetBillable, 0);
});

test("unreadable model, wrong route and failed reservation logging never open upstream", async (): Promise<void> => {
  let requests = 0;
  const upstream = createServer((_request, response): void => {
    requests++;
    response.end();
  });
  const routerPort = await listenOnTestPort(upstream);
  const gate = await startSonnetBudgetGate({
    routerPort,
    budget: new SonnetRequestBudget(0, () => {
      throw new Error("disk write failed");
    }),
  });
  try {
    for (const [suffix, body] of [
      ["/v1/messages", "{}"],
      ["/v1/messages", '{"model":"claude-sonnet-test"}'],
      ["/other", '{"model":"claude-sonnet-test"}'],
    ] satisfies readonly [string, string][]) {
      const r = await fetch(gate.baseUrl + suffix, { method: "POST", body });
      assert.notEqual(r.status, 200);
      await r.text();
    }
    assert.equal(requests, 0);
  } finally {
    await gate.close();
    upstream.closeAllConnections();
    await new Promise<void>((resolve) => upstream.close(() => resolve()));
  }
});
