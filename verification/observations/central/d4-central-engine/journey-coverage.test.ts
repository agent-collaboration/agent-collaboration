import { test } from "node:test";
import assert from "node:assert/strict";
import {
  requiredD4Checks,
  assessRecordedD4Coverage,
  type D4CheckOutcome,
} from "./journey-coverage.ts";

function recorded(shell: "tokio" | "workerd" | "staging"): Map<string, D4CheckOutcome> {
  return new Map(
    requiredD4Checks(shell).map((key): [string, D4CheckOutcome] => [
      key,
      { kind: "observed", evidence: [`fixture:${key}`] },
    ]),
  );
}

test("the local matrix preserves every commissioned journey, while target exceptions stay explicit", (): void => {
  const tokio = requiredD4Checks("tokio");
  const workerd = requiredD4Checks("workerd");
  const staging = requiredD4Checks("staging");
  for (const journey of "J1 J1b J1c J2 J2b J2c J3 J3b J4 J5 J5b J6 J6b J7 J7b J7c J8 J9 J10 J13 J14 J17 J18 J19 J20 J21 J22 J23 J25 J26 J27 J28 J29".split(
    " ",
  ))
    assert.ok(
      tokio.some((key): boolean => key.startsWith(`${journey}:`)),
      journey,
    );
  for (const journey of ["J5", "J13", "J18", "J15"])
    assert.equal(
      workerd.some((key): boolean => key.startsWith(`${journey}:`)),
      false,
      journey,
    );
  assert.ok(workerd.includes("J26:aged-row-at-boundary-not-24h-history"));
  assert.equal(workerd.includes("J26:24h-injected-history"), false);
  assert.ok(staging.includes("J14:same-sockets-and-attachments"));
  assert.ok(staging.includes("J15:one-real-hour"));
  assert.deepEqual(
    [...new Set(staging.map((key): string => key.split(":")[0] ?? ""))],
    ["J14", "J15"],
  );
  assert.equal(new Set(tokio).size, tokio.length);
});

test("new batch and notice-ack obligations cannot disappear behind old complete records", (): void => {
  const checks = recorded("workerd");
  checks.delete("J28:mailbox-full-item-alone-retries");
  checks.delete("J29:backoff-resends-from43");
  checks.delete("J29:applied45-retires-attempt");
  const result = assessRecordedD4Coverage({ shell: "workerd", checks });
  assert.equal(result.complete, false);
  assert.deepEqual(
    result.open.map((entry): string => entry.check),
    [
      "J28:mailbox-full-item-alone-retries",
      "J29:backoff-resends-from43",
      "J29:applied45-retires-attempt",
    ],
  );
});

test("empty or mostly completed records cannot close missing J22 carries or owner fault cuts", (): void => {
  const empty = assessRecordedD4Coverage({ shell: "tokio", checks: new Map() });
  assert.equal(empty.complete, false);
  const checks = recorded("tokio");
  checks.delete("J22:one-real-hour-no-unsolicited-notice-resend");
  checks.delete("J22:passive-alarm-cause-no-notice-wakes");
  checks.delete("J7:machine-status-cut");
  const result = assessRecordedD4Coverage({ shell: "tokio", checks });
  assert.equal(result.complete, false);
  assert.deepEqual(
    result.open.map((row) => row.check),
    [
      "J7:machine-status-cut",
      "J22:one-real-hour-no-unsolicited-notice-resend",
      "J22:passive-alarm-cause-no-notice-wakes",
    ],
  );
});

test("Tokio reconstruction coverage is required per applicable journey, not one ceremonial rebuild", (): void => {
  const checks = recorded("tokio");
  checks.delete("J13:rebuild-J23");
  const result = assessRecordedD4Coverage({ shell: "tokio", checks });
  assert.equal(result.complete, false);
  assert.deepEqual(
    result.open.map((row) => row.check),
    ["J13:rebuild-J23"],
  );
});

test("wrong-build and restored-positive evidence remain separate mandatory checks", (): void => {
  const checks = recorded("workerd");
  checks.set("J2c:wrong-build-semantic-failure", {
    kind: "open",
    reason: "seed did not reach fault",
  });
  checks.set("J26:restored-positive", { kind: "failed", reason: "retry remained stuck" });
  const result = assessRecordedD4Coverage({ shell: "workerd", checks });
  assert.equal(result.complete, false);
  assert.deepEqual(result.open, [
    { check: "J2c:wrong-build-semantic-failure", reason: "seed did not reach fault" },
  ]);
  assert.deepEqual(result.failed, [
    { check: "J26:restored-positive", reason: "retry remained stuck" },
  ]);
});

test("unknown or wrong-shell check names and evidence-free observed records are rejected", (): void => {
  assert.throws((): void => {
    assessRecordedD4Coverage({
      shell: "workerd",
      checks: new Map([["J5:100-one-batch", { kind: "observed", evidence: ["fixture"] }]]),
    });
  }, /unregistered/);
  const checks = recorded("tokio");
  checks.set("J1:alarms-disabled", { kind: "observed", evidence: [] });
  assert.throws((): void => {
    assessRecordedD4Coverage({ shell: "tokio", checks });
  });
});

test("complete bookkeeping still carries runtimeQualified false instead of product acceptance", (): void => {
  const result = assessRecordedD4Coverage({ shell: "tokio", checks: recorded("tokio") });
  assert.equal(result.complete, true);
  assert.equal(result.runtimeQualified, false);
});
