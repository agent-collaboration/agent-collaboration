// The SQLx Durable Object driver's scenario Worker (moved with the driver from
// agent-fleet-sync) under the pinned workerd: every scenario it lists runs inside a
// real SQLite-backed Durable Object and must answer ok. Replaces the former
// `wrangler dev` script and Rust HTTP driver.
import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { WorkerdCentral, driverScenarioBundle } from "./workerd-central.ts";

let worker: WorkerdCentral;

before(async () => {
  worker = await WorkerdCentral.start(driverScenarioBundle());
});
after(async () => {
  await worker.close();
});

test("every driver scenario passes against a SQLite-backed Durable Object", async () => {
  const listed: unknown = await (await fetch(`${worker.origin}/`)).json();
  assert.ok(Array.isArray(listed) && listed.length > 0, "the Worker lists no scenarios");
  const failures: string[] = [];
  // All of them, then one failure listing every scenario that failed.
  for (const name of listed as string[]) {
    const response = await fetch(`${worker.origin}/${name}`);
    const body = await response.text();
    try {
      const outcome = JSON.parse(body) as { ok: boolean; error?: string };
      if (!outcome.ok) failures.push(`${name}: ${outcome.error ?? "no error given"}`);
    } catch {
      failures.push(`${name}: ${body}`);
    }
  }
  assert.deepEqual(failures, []);
});
