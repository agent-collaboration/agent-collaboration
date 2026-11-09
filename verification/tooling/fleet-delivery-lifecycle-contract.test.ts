import assert from "node:assert/strict";
import { test } from "node:test";
import {
  deliveryLifecycleCases,
  requireDeliveryDiscovery,
  requireDeliveryOutcomes,
} from "./fleet-delivery-lifecycle-contract.ts";

function outcomeLog(status: string = "PASS"): string {
  return deliveryLifecycleCases
    .map((row) => `${status} [ 0.010s] (1/7) ${row.binary} ${row.test}`)
    .join("\n");
}

test("requires seven exact passing final identities, with nextest counters or without them", () => {
  assert.equal(requireDeliveryOutcomes(outcomeLog()).length, 7);
  assert.equal(requireDeliveryOutcomes(outcomeLog().replaceAll("(1/7) ", "")).length, 7);
});

test("zero matches, missing cases, repeated final outcomes and required skips cannot pass", () => {
  assert.throws(() => requireDeliveryOutcomes("test result: ok. 0 passed"));
  assert.throws(() => requireDeliveryOutcomes(outcomeLog().split("\n").slice(1).join("\n")));
  assert.throws(() => requireDeliveryOutcomes(`${outcomeLog()}\n${outcomeLog().split("\n")[0]}`));
  assert.throws(() => requireDeliveryOutcomes(outcomeLog("SKIP")));
});

test("a failure cannot be hidden by a successful package banner or other cases", () => {
  assert.throws(() =>
    requireDeliveryOutcomes(`${outcomeLog().replace("PASS", "FAIL")}\ntest result: ok.`),
  );
  assert.throws(() =>
    requireDeliveryOutcomes(`${outcomeLog()}\nPASS [ 0.1s] fleet-mail-delivery unexpected_case`),
  );
  assert.equal(
    requireDeliveryOutcomes(`${outcomeLog()}\nSKIP [ ] fleet-mail-delivery filtered_case`).length,
    7,
  );
});

function discovery(): {
  "rust-suites": Record<
    string,
    {
      "binary-path": string;
      testcases: Record<string, { ignored: boolean; "filter-match": { status: string } }>;
    }
  >;
} {
  const suites: ReturnType<typeof discovery>["rust-suites"] = {};
  for (const row of deliveryLifecycleCases) {
    const suite = suites[row.binary] ?? { "binary-path": `/owned/${row.binary}`, testcases: {} };
    suite.testcases[row.test] = { ignored: false, "filter-match": { status: "matches" } };
    suites[row.binary] = suite;
  }
  return { "rust-suites": suites };
}

test("discovery refuses an ignored, missing or wrong-binary identity before execution", () => {
  assert.equal(requireDeliveryDiscovery(discovery()).length, 2);
  const ignored = discovery();
  const firstSuite = Object.values(ignored["rust-suites"])[0];
  assert(firstSuite);
  const firstCase = Object.values(firstSuite.testcases)[0];
  assert(firstCase);
  firstCase.ignored = true;
  assert.throws(() => requireDeliveryDiscovery(ignored));
  assert.throws(() => requireDeliveryDiscovery({ "rust-suites": {} }));
  const wrong = discovery();
  const suite = wrong["rust-suites"]["fleet-mail-delivery"];
  assert(suite);
  delete wrong["rust-suites"]["fleet-mail-delivery"];
  wrong["rust-suites"].other_package = suite;
  assert.throws(() => requireDeliveryDiscovery(wrong));
});
