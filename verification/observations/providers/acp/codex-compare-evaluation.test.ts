import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  PROVIDER_USAGE_FILE,
  RUN_RECORD_FILE,
  compareRuns,
  evaluateRun,
  loadCompareRun,
  providerUsageFromRouterTurns,
} from "./codex-compare-evaluation.ts";
import type { CompareRunRecord, LoadedRun, ProviderUsageFile } from "./codex-compare-evaluation.ts";
import { APPROVAL_OUTCOME_FILE } from "./codex-approval-outcome.ts";
import { SCRIPTED, scriptRun } from "./codex-compare-fixtures.ts";
import type { ScriptRunOptions, Wire } from "./codex-compare-fixtures.ts";
import { reduceTurnSkeleton } from "./turn-skeleton.ts";

function runFrom(
  wire: Wire,
  kind: CompareRunRecord["run_kind"],
  providerUsage: ProviderUsageFile | null,
): LoadedRun {
  return {
    record: {
      run_kind: kind,
      wire_recording: "wire.jsonl",
      scripted_settings: SCRIPTED,
      agent_approval_outcomes: null,
    },
    skeleton: reduceTurnSkeleton(wire.build()),
    providerUsage,
  };
}

const PROVIDER_1M = [{ turn_ordinal: 0, used: 4000, size: 1_000_000 }];
const reference = (): LoadedRun =>
  runFrom(scriptRun({ turnId: "R1", window: 258_000 }), "codex-reference", null);
const candidate = (
  options: ScriptRunOptions = { turnId: "C1", window: 1_000_000 },
  usage: ProviderUsageFile | null = PROVIDER_1M,
): LoadedRun => runFrom(scriptRun(options), "router-candidate", usage);

test("matching runs with different turn ids and window sizes pass", () => {
  const verdict = compareRuns(reference(), candidate());
  assert.deepEqual(verdict, { status: "pass", reference: [], candidate: [], crossRun: [] });
});

test("a dropped steer receipt fails the candidate under k3", () => {
  const verdict = compareRuns(
    reference(),
    candidate({ turnId: "C1", window: 1_000_000, dropSteerReceipt: true }),
  );
  assert.equal(verdict.status, "fail");
  assert.ok(verdict.candidate.some((violation) => violation.rule === "k3"));
});

test("a duplicated receipt fails the candidate under k2", () => {
  const verdict = compareRuns(
    reference(),
    candidate({ turnId: "C1", window: 1_000_000, duplicateSteerReceipt: true }),
  );
  assert.equal(verdict.status, "fail");
  assert.ok(verdict.candidate.some((violation) => violation.rule === "k2"));
});

test("an agent item before the opening user item fails the candidate under k2", () => {
  const verdict = compareRuns(
    reference(),
    candidate({ turnId: "C1", window: 1_000_000, agentBeforeUser: true }),
  );
  assert.equal(verdict.status, "fail");
  assert.ok(
    verdict.candidate.some(
      (violation) => violation.rule === "k2" && violation.detail.includes("before the opening"),
    ),
  );
});

test("a 770K window where the provider reported 1M fails the usage rule", () => {
  const verdict = compareRuns(reference(), candidate({ turnId: "C1", window: 770_000 }));
  assert.equal(verdict.status, "fail");
  assert.deepEqual(
    verdict.candidate.map((violation) => violation.rule),
    ["usage"],
  );
  assert.match(verdict.candidate[0]?.detail ?? "", /770000.*1000000/u);
});

test("a candidate with no provider usage report cannot pass", () => {
  for (const usage of [null, [] as ProviderUsageFile]) {
    const verdict = compareRuns(reference(), candidate(undefined, usage));
    assert.equal(verdict.status, "fail", String(usage));
    assert.ok(verdict.candidate.some((violation) => violation.rule === "usage"));
  }
});

test("a provider turn the CLI never saw is reported", () => {
  const verdict = compareRuns(
    reference(),
    candidate(undefined, [...PROVIDER_1M, { turn_ordinal: 1, used: 1, size: 1_000_000 }]),
  );
  assert.equal(verdict.status, "fail");
  assert.match(
    verdict.candidate.map((violation) => violation.detail).join("\n"),
    /missing-turn-1/u,
  );
});

test("a lost model change fails the correspondence rule", () => {
  const verdict = compareRuns(
    reference(),
    candidate({ turnId: "C1", window: 1_000_000, settings: ["old-model", "low", null] }),
  );
  assert.equal(verdict.status, "fail");
  assert.ok(
    verdict.crossRun.some(
      (violation) => violation.rule === "k5" && violation.detail.includes("candidate settings"),
    ),
  );
});

test("a tier that does not clear fails the correspondence rule", () => {
  const verdict = compareRuns(
    reference(),
    candidate({ turnId: "C1", window: 1_000_000, settings: ["gpt-x", "low", "fast"] }),
  );
  assert.equal(verdict.status, "fail");
  assert.ok(verdict.crossRun.some((violation) => violation.rule === "k5"));
});

test("a rule real Codex breaks is reported as contradicted by the reference, not relaxed", () => {
  const broken = runFrom(
    scriptRun({ turnId: "R1", window: 258_000, dropSteerReceipt: true }),
    "codex-reference",
    null,
  );
  const verdict = compareRuns(broken, candidate());
  assert.equal(verdict.status, "rule-contradicted-by-reference");
  assert.ok(verdict.reference.some((violation) => violation.rule === "k3"));
  // Even a candidate that also breaks the rule does not pass, and is not excused.
  const both = compareRuns(
    broken,
    candidate({ turnId: "C1", window: 1_000_000, dropSteerReceipt: true }),
  );
  assert.equal(both.status, "rule-contradicted-by-reference");
  assert.ok(both.candidate.length > 0);
});

test("the reference run's usage is not checked against a provider report", () => {
  const evaluation = evaluateRun(reference());
  assert.equal(evaluation.usageChecked, false);
  assert.deepEqual(evaluation.violations, []);
});

test("run kinds are enforced", () => {
  assert.throws(() => compareRuns(candidate(), candidate()), /reference run must be/u);
  assert.throws(() => compareRuns(reference(), reference()), /candidate run must be/u);
});

test("provider usage is read from each turn's last Usage item payload", () => {
  const usage = providerUsageFromRouterTurns([
    { items: [{ kind: "agentMessage", text: "hi" }] },
    {
      items: [
        { kind: "usage", text: JSON.stringify({ used: 1, size: 5 }) },
        { kind: "usage", text: JSON.stringify({ used: 4000, size: 1_000_000, cost: null }) },
      ],
    },
  ]);
  assert.deepEqual(usage, [{ turn_ordinal: 1, used: 4000, size: 1_000_000 }]);
  assert.throws(
    () => providerUsageFromRouterTurns([{ items: [{ kind: "usage", text: null }] }]),
    /no payload/u,
  );
  assert.throws(() =>
    providerUsageFromRouterTurns([{ items: [{ kind: "usage", text: '{"used":1}' }] }]),
  );
});

test("a run directory is loaded from its record, recording and usage file", () => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "cmp-eval-")));
  try {
    const directory = join(root, "run");
    mkdirSync(directory);
    const wire = scriptRun({ turnId: "C1", window: 1_000_000 });
    writeFileSync(
      join(directory, "wire.jsonl"),
      wire
        .build()
        .map((frame) => JSON.stringify(frame))
        .join("\n"),
    );
    const record: CompareRunRecord = {
      run_kind: "router-candidate",
      wire_recording: "wire.jsonl",
      scripted_settings: SCRIPTED,
      agent_approval_outcomes: null,
    };
    writeFileSync(join(directory, RUN_RECORD_FILE), JSON.stringify(record));
    assert.equal(loadCompareRun(directory).providerUsage, null);
    writeFileSync(join(directory, PROVIDER_USAGE_FILE), JSON.stringify(PROVIDER_1M));
    const loaded = loadCompareRun(directory);
    assert.deepEqual(evaluateRun(loaded).violations, []);
    // A second CLI connection is refused: one-window runs only.
    const extra = [...wire.build(), { ...wire.build()[0], seq: 999, connection: 1 }];
    writeFileSync(
      join(directory, "wire.jsonl"),
      extra.map((frame) => JSON.stringify(frame)).join("\n"),
    );
    assert.throws(() => loadCompareRun(directory), /2 CLI connections/u);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("an approval outcome read from the agent side feeds k5, and a wrong one fails it", () => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "cmp-approval-")));
  try {
    const directory = join(root, "run");
    mkdirSync(directory);
    const wire = scriptRun({ turnId: "C1", window: 1_000_000 });
    writeFileSync(
      join(directory, "wire.jsonl"),
      wire
        .build()
        .map((frame) => JSON.stringify(frame))
        .join("\n"),
    );
    const record: CompareRunRecord = {
      run_kind: "router-candidate",
      wire_recording: "wire.jsonl",
      scripted_settings: SCRIPTED,
      agent_approval_outcomes: null,
    };
    writeFileSync(join(directory, RUN_RECORD_FILE), JSON.stringify(record));
    assert.equal(loadCompareRun(directory).record.agent_approval_outcomes, null);
    const outcome = {
      case: "allow-once",
      request_id: "r1",
      pending_state: "pendingClientDecision",
      final_state: "decided",
      file_present: true,
      agent_outcome: "accept",
      violations: [],
    };
    writeFileSync(join(directory, APPROVAL_OUTCOME_FILE), JSON.stringify(outcome));
    assert.deepEqual(loadCompareRun(directory).record.agent_approval_outcomes, ["accept"]);
    // A timeout is not expressible as a CLI decision: the run stays unpaired.
    writeFileSync(
      join(directory, APPROVAL_OUTCOME_FILE),
      JSON.stringify({ ...outcome, case: "timeout", agent_outcome: "timedOut" }),
    );
    assert.equal(loadCompareRun(directory).record.agent_approval_outcomes, null);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
