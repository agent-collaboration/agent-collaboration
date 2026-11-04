import { createFixtureDirectory } from "../test-support/fixture-directory.ts";
import assert from "node:assert/strict";
import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test, type TestContext } from "node:test";
import { readJourneyObservationPlan } from "./acp-journey-observation.ts";

// A plan as its author writes it: plain JSON integers for host_pid and image_count.
function planText(imageCount: string): string {
  return `{
  "repository": "/work/repo",
  "source_head": "fixture",
  "build_manifest": "/work/build.json",
  "host_pid": 4242,
  "host_binary": "/work/host",
  "host_sha256": "fixture",
  "router_binary": "/work/router",
  "router_sha256": "fixture",
  "codex_cli": "/work/codex",
  "codex_sha256": "fixture",
  "collaboration_cli": "/work/agent-collaboration",
  "service_directory": "/work/service",
  "endpoint_id": "cursor-local",
  "session_id": "fixture-session",
  "evidence": "/work/evidence",
  "native_thread_id": "5756efda-4d21-59ce-aef2-2b68a02a6598",
  "cwd": "/work/repo",
  "mode": "idle-bind",
  "checks": [
    {
      "kind": "skill-effect",
      "submission": {
        "original_text": "$fixture_skill ALPHA",
        "selected": ["fixture_skill"],
        "image_count": ${imageCount},
        "effect_marker": "ACP_FIXTURE_MARKER"
      }
    }
  ]
}`;
}

function writePlan(context: TestContext, text: string): string {
  const root = createFixtureDirectory("acp-journey-plan");
  context.after(() => rmSync(root, { recursive: true, force: true }));
  const path = join(root, "observer-plan.json");
  writeFileSync(path, text);
  return path;
}

test("the CLI reads an author-written plan's integers as ordinary numbers", (context): void => {
  const plan = readJourneyObservationPlan(writePlan(context, planText("1")));
  const [check] = plan.checks;
  assert.equal(check?.kind, "skill-effect");
  assert.equal(check.kind === "skill-effect" ? check.submission.image_count : null, 1);
  assert.equal(plan.host_pid, 4242);
});

test("the plan file still refuses conflicting keys, unsafe integers and quoted counts", (context): void => {
  const conflicting = planText("1").replace(
    '"session_id": "fixture-session",',
    '"session_id": "fixture-session",\n  "session_id": "other-session",',
  );
  assert.throws(() => readJourneyObservationPlan(writePlan(context, conflicting)), /Duplicate key/);
  for (const count of ["9007199254740993", "1.5", '"1"', "-1"])
    assert.throws(
      () => readJourneyObservationPlan(writePlan(context, planText(count))),
      /image_count/,
      count,
    );
});
