import assert from "node:assert/strict";
import test from "node:test";
import { centralAutomationsPlanSchema } from "../central-automations-plan.ts";

const declared = {
  repository: "/owned/router",
  sourceHead: "a".repeat(40),
  executable: {
    path: "/owned/router/target/debug/examples/central-automations-rig",
    sha256: "b".repeat(64),
    identifier: "dev.shravansunder.test.central-automations-rig",
    team: "974QD84WVC",
  },
  directory: "/owned/router/tmp/r",
  evidence: "/owned/router/tmp/evidence",
  model: "claude-haiku-5-5",
  maxBudgetUsd: 0.1,
  purpose: "bootstrap-only",
};

test("the bootstrap accepts two local roots without an SSH or model-turn requirement", (): void => {
  assert.deepEqual(centralAutomationsPlanSchema.parse(declared), declared);
});

test("runtime and evidence are confined to separate directories in the assigned checkout", (): void => {
  for (const directory of [
    "/tmp/rogue",
    "/owned/router/tmp",
    "/owned/router/tmp/../rogue",
    "/owned/router/tmp-other/run",
  ]) {
    assert.equal(
      centralAutomationsPlanSchema.safeParse({ ...declared, directory }).success,
      false,
      directory,
    );
  }
  for (const evidence of [
    declared.directory,
    `${declared.directory}/evidence`,
    `${declared.directory}/..cache`,
    "/owned/router",
  ]) {
    assert.equal(
      centralAutomationsPlanSchema.safeParse({ ...declared, evidence }).success,
      false,
      evidence,
    );
  }
});

test("a different binary, model, oversized budget, or implicit product observation is refused", (): void => {
  for (const changed of [
    {
      executable: {
        ...declared.executable,
        path: "/other/target/debug/examples/central-automations-rig",
      },
    },
    { model: "gpt-6-luna" },
    { maxBudgetUsd: 1 },
    { purpose: "all-rows-pass" },
    { directory: `/owned/router/tmp/${"x".repeat(80)}` },
  ]) {
    assert.equal(
      centralAutomationsPlanSchema.safeParse({ ...declared, ...changed }).success,
      false,
    );
  }
});
