import assert from "node:assert/strict";
import { test } from "node:test";
import { d4OfflineHarnessPlanSchema, d1PlanForOfflineCustody } from "./offline-harness-plan.ts";

const artifact = {
  path: "/declared/frozen/executable",
  sha256: "a".repeat(64),
  identifier: "declared.debug",
  team: "974QD84WVC",
};
const declaration = {
  suite: "d4-offline-custody",
  shell: "tokio",
  sourceHead: "a".repeat(40),
  apparatusHead: "b".repeat(40),
  sourceArchive: { path: "/declared/source.tar", sha256: "c".repeat(64) },
  repository: "/declared/frozen-source",
  driver: artifact,
  cli: artifact,
  native: { ...artifact, version: "codex-cli 0.166.0" },
  evidenceDirectory: "/declared/frozen-source/tmp/d1-verify/evidence",
  runtimeDirectory: "/private/tmp/d1v-abc123",
  debugProxyOrigin: "http://127.0.0.1:18787",
  deadline: "2033-05-18T03:33:20Z",
  profile: "dev",
};

test("D4 offline target requires exact source, signed artifacts, qualified native and short isolated runtime", (): void => {
  assert.equal(d4OfflineHarnessPlanSchema.safeParse(declaration).success, true);
  for (const invalid of [
    { ...declaration, sourceHead: "unknown" },
    { ...declaration, apparatusHead: "unknown" },
    { ...declaration, driver: { ...artifact, team: "ad-hoc" } },
    { ...declaration, native: { ...artifact, version: "codex-cli 0.166.1" } },
    { ...declaration, native: { ...artifact, version: "codex-cli 0.166.0-unqualified" } },
    { ...declaration, runtimeDirectory: undefined },
    { ...declaration, shell: "workerd" },
    { ...declaration, profile: "release" },
  ])
    assert.equal(d4OfflineHarnessPlanSchema.safeParse(invalid).success, false);
});
test("offline custody cannot gain model authority or mix a legacy drive through its declaration", (): void => {
  for (const extra of [
    { allowModelInput: true },
    { materialization: {} },
    { twoProjects: true },
    { queueScenario: "online-held" },
  ])
    assert.equal(d4OfflineHarnessPlanSchema.safeParse({ ...declaration, ...extra }).success, false);
  const selected = d1PlanForOfflineCustody(d4OfflineHarnessPlanSchema.parse(declaration));
  assert.equal(selected.queueScenario, "none");
  assert.equal(selected.twoProjects, false);
  assert.equal(selected.materialization, undefined);
});
