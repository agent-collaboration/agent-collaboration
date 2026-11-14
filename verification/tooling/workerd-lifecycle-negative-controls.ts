// Explicit negative controls own their witnesses and settle them before returning.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "../..");
const evidence = resolve(root, "tmp/2026-11-14-workerd-lifecycle/negative-controls");
mkdirSync(evidence, { recursive: true });
const base = [
  "nextest",
  "run",
  "--locked",
  "-p",
  "collaboration-central-cloudflare",
  "--features",
  "test-support",
  "--test",
  "integration",
  "--run-ignored",
  "only",
  "--config-file",
  "crates/collaboration-central-cloudflare/tests/support/lifecycle-nextest.toml",
  "--color",
  "never",
  "--success-output",
  "immediate",
];
for (const mode of ["timeout", "leak"]) {
  const selector =
    mode === "timeout" ? "lifecycle_child_owns_real_workerd" : "lifecycle_leak_negative_control";
  const result = spawnSync(
    "cargo",
    [...base, "-E", `test(=${`workspace_requests::workerd_host::lifecycle_tests::${selector}`})`],
    {
      cwd: root,
      env: { ...process.env, WORKERD_LIFECYCLE_CHILD: mode },
      encoding: "utf8",
    },
  );
  const output = result.stdout + result.stderr;
  writeFileSync(resolve(evidence, `${mode}.log`), output);
  if (result.error !== undefined) throw result.error;
  if (mode === "timeout") {
    assert.equal(result.status, 100);
    assert.match(output, /1 timed out/);
    const match = output.match(/WORKERD_LIFECYCLE_READY (\d+) (\d+)/);
    assert.ok(match !== null, "zero-match or pre-readiness failure is not a timeout proof");
    for (const raw of match.slice(1))
      assert.throws(() => process.kill(Number(raw), 0), { code: "ESRCH" });
  } else {
    const match = output.match(/WORKERD_LIFECYCLE_LEAK_PID (\d+)/);
    assert.ok(match !== null, "zero-match cannot establish a leak");
    const pid = Number(match[1]);
    try {
      assert.equal(result.status, 0);
      assert.match(output, /^\s*LEAK\s/m);
      const gate = spawnSync("/bin/bash", [
        "-c",
        "if grep -Eq '^[[:space:]]*LEAK[[:space:]]' \"$1\"; then exit 1; fi",
        "gate",
        resolve(evidence, "leak.log"),
      ]);
      assert.equal(gate.status, 1, "genuine nextest LEAK must reject");
    } finally {
      // The deliberately leaked witness is a plain Node child, never a bridge.
      process.kill(pid, "SIGTERM");
    }
  }
  writeFileSync(
    resolve(evidence, `${mode}.json`),
    JSON.stringify(
      {
        command: [
          "cargo",
          ...base,
          "-E",
          `test(=${`workspace_requests::workerd_host::lifecycle_tests::${selector}`})`,
        ],
        exit: result.status,
      },
      null,
      2,
    ) + "\n",
  );
}
