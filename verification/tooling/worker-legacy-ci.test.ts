import assert from "node:assert/strict";
import { spawnSync, type SpawnSyncReturns } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

const workflow = readFileSync(new URL("../../.github/workflows/ci.yml", import.meta.url), "utf8");
const legacyCases = [
  "composed_scope_workerd/composed-copy-suite",
  "composed_scope_workerd/runtime-controls",
  "session_scope_workerd/forward-real-timer",
  "composed_scope_workerd/composed-transactions",
  "composed_scope_workerd/composed-numeric",
  "composed_scope_workerd/composed-fleet-conflict",
  "step_scope_workerd",
  "bootstrap_scope_workerd",
  "session_scope_workerd/access-sessions",
  "session_scope_workerd/access-carryover",
  "session_scope_workerd/session-carryover",
  "session_scope_workerd/forward-boundary",
  "session_scope_workerd/forward-overlap",
  "session_scope_workerd/forward-state",
  "session_scope_workerd/forward-terminal",
  "session_scope_workerd/mailbox-identity",
  "session_scope_workerd/mailbox-capacity",
  "session_scope_workerd/mailbox-cancel",
  "session_scope_workerd/fleet-mail-expiry",
  "session_scope_workerd/mailbox-held-retention",
  "session_scope_workerd/mailbox-report-terminal",
];

function extractWorkflowStep(name: string): string {
  const marker = `      - name: ${name}\n`;
  const start = workflow.indexOf(marker);
  assert.notEqual(start, -1, `missing workflow step ${name}`);
  const next = workflow.indexOf("      - name:", start + marker.length);
  const step = workflow.slice(start, next === -1 ? undefined : next);
  const run = step.indexOf("        run: |\n");
  assert.notEqual(run, -1, `missing run block for ${name}`);
  return step
    .slice(run + "        run: |\n".length)
    .split("\n")
    .map((line) => line.replace(/^          /, ""))
    .join("\n");
}

// Only external build/scenario tools stand in. The workflow's actual Bash,
// argument ordering, redirects, child statuses, waits and log replay stay real.
const standInTools = `
worker-build() { return 0; }
node() { return 0; }
date() { printf '1000.000000000\\n'; }
cargo() {
  local example='' scenario=''
  while (($#)); do
    case "$1" in
      --example) example="$2"; shift 2 ;;
      --) shift; break ;;
      *) shift ;;
    esac
  done
  if [[ "$example" == composed_scope_workerd ]]; then
    scenario="$1"; shift
  elif [[ "$example" == session_scope_workerd ]]; then
    scenario="\${@: -1}"
  fi
  [[ "$1" == node && "$2" == "$RUNNER_TEMP/central-node-tools/node_modules/miniflare/dist/src/index.js" ]] || return 91
  local case_name="$example\${scenario:+/$scenario}"
  printf '%s|%s|%s\\n' "$case_name" "$3" "$4" >> "$RUNNER_TEMP/invocations"
  printf 'SCENARIO %s\\n' "$case_name"
  if [[ "$case_name" == "$FAIL_CASE" ]]; then
    printf 'FORCED_FAILURE %s\\n' "$case_name"
    return 23
  fi
}
`;

function runLegacyStep(props: {
  root: string;
  script: string;
  failingCase: string;
}): SpawnSyncReturns<string> {
  return spawnSync("bash", ["-s"], {
    input: `${standInTools}\n${props.script}`,
    env: { ...process.env, RUNNER_TEMP: props.root, FAIL_CASE: props.failingCase },
    encoding: "utf8",
    timeout: 20_000,
  });
}

function readRecordedInvocations(root: string): string[] {
  const path = join(root, "invocations");
  return existsSync(path) ? readFileSync(path, "utf8").trim().split("\n") : [];
}

test("the extracted legacy step executes all 21 isolated cases and reports every duration", (): void => {
  const root = mkdtempSync(join(tmpdir(), "worker-legacy-ci-"));
  try {
    const result = runLegacyStep({
      root,
      script: extractWorkflowStep("Prove legacy workerd scenarios with two isolated processes"),
      failingCase: "",
    });
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    const records = readRecordedInvocations(root).map((line) => line.split("|"));
    assert.deepEqual(records.map(([name]) => name).sort(), [...legacyCases].sort());
    assert.equal(new Set(records.map(([, persistence]) => persistence)).size, 21);
    assert.deepEqual(
      records.map(([, , port]) => Number(port)).sort((left, right) => left - right),
      Array.from({ length: 21 }, (_, index) => 8957 + index),
    );
    for (const [, persistence] of records) {
      assert.ok(persistence);
      assert.ok(persistence.startsWith(`${root}/central-workerd-`));
      assert.ok(!persistence.slice(root.length + 1).includes("/"));
    }
    const timings = [...result.stdout.matchAll(/^WORKERD_TEST\t\d+\.\d{3}\t(.+)$/gm)];
    assert.deepEqual(timings.map((match) => match[1]).sort(), [...legacyCases].sort());
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a failed case rejects its pair and the following recovery step prints retained logs", (): void => {
  const root = mkdtempSync(join(tmpdir(), "worker-legacy-ci-failure-"));
  try {
    const result = runLegacyStep({
      root,
      script: extractWorkflowStep("Prove legacy workerd scenarios with two isolated processes"),
      failingCase: "session_scope_workerd/forward-real-timer",
    });
    assert.equal(result.status, 1, `${result.stdout}\n${result.stderr}`);
    assert.equal(readRecordedInvocations(root).length, 4);
    assert.match(result.stdout, /FORCED_FAILURE session_scope_workerd\/forward-real-timer/);
    assert.match(result.stdout, /SCENARIO composed_scope_workerd\/composed-transactions/);
    assert.match(
      workflow,
      /name: Print legacy scenario logs after failure or cancellation\n\s+if: failure\(\) \|\| cancelled\(\)/,
    );
    const recovery = runLegacyStep({
      root,
      script: extractWorkflowStep("Print legacy scenario logs after failure or cancellation"),
      failingCase: "",
    });
    assert.equal(recovery.status, 0, recovery.stderr);
    assert.match(recovery.stdout, /FORCED_FAILURE session_scope_workerd\/forward-real-timer/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
