import assert from "node:assert/strict";
import { createServer } from "node:http";
import { test } from "node:test";
import { listenOnTestPort } from "../../mcp-control/test-support/loopback-test-port.ts";
import {
  SonnetRequestBudget,
  startSonnetBudgetGate,
  type BudgetEvent,
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
    assert.deepEqual(budget.summary(), { sonnetUsed: 3, byModel: { "claude-sonnet-test": 3 } });
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
  assert.equal(budget.reserve("claude-haiku-classifier"), true);
  assert.equal(budget.reserve("claude-sonnet-test"), true);
  assert.equal(budget.reserve("claude-sonnet-other"), false);
  assert.deepEqual(budget.summary(), {
    sonnetUsed: 3,
    byModel: { "claude-haiku-classifier": 1, "claude-sonnet-test": 1 },
  });
  assert.equal(events.at(-1)?.kind, "refused");
  assert.throws(() => new SonnetRequestBudget(4, () => {}), /invalid/);
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
