import assert from "node:assert/strict";
import { stripVTControlCharacters } from "node:util";
import { z } from "zod";

export const deliveryLifecycleCases = [
  {
    binary: "fleet-mail-delivery",
    test: "relay_schema::tests::relay_retirement_preserves_relay_api_calls_across_reopen",
  },
  {
    binary: "fleet-mail-delivery",
    test: "relay_schema::tests::relay_retirement_is_transactional_and_leaves_other_tables_unchanged",
  },
  {
    binary: "fleet-mail-delivery",
    test: "relay_schema::tests::relay_retirement_accepts_a_store_with_no_delivery_tables",
  },
  {
    binary: "fleet-mail-delivery",
    test: "relay_schema::tests::machine_startup_preserves_custody_bytes_and_removal_still_cleans_both_homes",
  },
  {
    binary: "fleet-mail-delivery::integration",
    test: "local_cancel_routes::push_promotes_a_prevented_local_original_and_reports_once_across_reopen",
  },
  {
    binary: "fleet-mail-delivery::integration",
    test: "local_cancel_routes::failed_preserved_outcome_projection_rolls_back_push_promotion_and_report_debt",
  },
  {
    binary: "fleet-mail-delivery::integration",
    test: "relay_outbox::unadmitted_cancel_reports_return_typed_refusal_without_any_transaction_write",
  },
] as const;

const discoverySchema = z.looseObject({
  "rust-suites": z.record(
    z.string(),
    z.looseObject({
      "binary-path": z.string().min(1),
      testcases: z.record(
        z.string(),
        z.looseObject({
          ignored: z.boolean(),
          "filter-match": z.looseObject({ status: z.string() }),
        }),
      ),
    }),
  ),
});

export function requireDeliveryDiscovery(value: unknown): readonly string[] {
  const suites = discoverySchema.parse(value)["rust-suites"];
  const selected: string[] = [];
  const binaries = new Set<string>();
  for (const [binary, suite] of Object.entries(suites)) {
    for (const [name, test] of Object.entries(suite.testcases)) {
      if (test["filter-match"].status !== "matches") continue;
      assert(!test.ignored, `required delivery case is ignored: ${name}`);
      selected.push(`${binary}/${name}`);
      binaries.add(suite["binary-path"]);
    }
  }
  assert.deepEqual(
    selected.sort(),
    deliveryLifecycleCases.map((row) => `${row.binary}/${row.test}`).sort(),
    "delivery discovery must select exactly seven required identities",
  );
  return [...binaries];
}

export interface DeliveryOutcome {
  readonly binary: string;
  readonly test: string;
  readonly status: string;
}

export function requireDeliveryOutcomes(rawLog: string): readonly DeliveryOutcome[] {
  const outcomes: DeliveryOutcome[] = [];
  for (const line of stripVTControlCharacters(rawLog).split("\n")) {
    const match = line.match(
      /^\s*(PASS|FAIL|SKIP|TIMEOUT|LEAK|EXECFAIL|FLAKY)\s+\[[^\]]*\]\s+(?:\([^)]*\)\s+)?(\S+)\s+(.+?)\s*$/,
    );
    if (!match) continue;
    const status = z.string().parse(match[1]);
    const binary = z.string().parse(match[2]);
    const test = z.string().parse(match[3]);
    const required = deliveryLifecycleCases.some(
      (row) => row.binary === binary && row.test === test,
    );
    if (!required) {
      assert.equal(status, "SKIP", `unexpected executed delivery case: ${binary}/${test}`);
      continue;
    }
    outcomes.push({ binary, test, status });
  }
  for (const row of deliveryLifecycleCases) {
    const matching = outcomes.filter(
      (outcome) => outcome.binary === row.binary && outcome.test === row.test,
    );
    assert.equal(matching.length, 1, `delivery case must finish exactly once: ${row.test}`);
    assert.equal(matching[0]?.status, "PASS", `delivery case did not pass: ${row.test}`);
  }
  assert.equal(outcomes.length, 7);
  return outcomes;
}
