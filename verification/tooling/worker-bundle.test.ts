import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { z } from "zod";
import { checkDeployTemplate, checkWorkerBundle } from "./check-central-worker-bundle.ts";

const schema = z.object({
  cargo: z.string(),
  readme: z.string(),
  config: z.string(),
  tooling: z.string(),
  cases: z.array(
    z.object({
      mode: z.enum(["production", "faults"]),
      content: z.string(),
      expected: z.object({ status: z.number(), stdout: z.string(), stderr: z.string() }),
    }),
  ),
});
const baseline = schema.parse(
  JSON.parse(
    readFileSync(new URL("./fixtures/worker-bundle-baselines.json", import.meta.url), "utf8"),
  ),
);

for (const [index, entry] of baseline.cases.entries()) {
  test(`Worker guard matches original exit and output, ${entry.mode} case ${index}`, (): void => {
    const root = mkdtempSync(join(tmpdir(), "worker-bundle-guard-"));
    try {
      const worker = join(root, "crates/collaboration-central-cloudflare");
      mkdirSync(join(worker, "build"), { recursive: true });
      writeFileSync(join(root, "Cargo.toml"), baseline.cargo);
      writeFileSync(join(worker, "README.md"), baseline.readme);
      writeFileSync(join(worker, "cloudflare.config.ts"), baseline.config);
      writeFileSync(join(worker, "wrangler.config.ts"), baseline.tooling);
      writeFileSync(join(worker, "build/index.js"), entry.content);
      writeFileSync(join(worker, "build/index_bg.wasm"), "");
      const result = checkWorkerBundle(entry.mode, root);
      assert.deepEqual(
        { status: result.status, stdout: `${result.message}\n`, stderr: "" },
        entry.expected,
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}

test("deploy templates refuse public routes, private values, and rebundling", (): void => {
  assert.equal(checkDeployTemplate(baseline.config, baseline.tooling), undefined);
  assert.equal(
    checkDeployTemplate(
      baseline.config.replace("workersDev: false", "workersDev: true"),
      baseline.tooling,
    ),
    "Worker deploy template must set workersDev: false",
  );
  assert.equal(
    checkDeployTemplate(`${baseline.config}, accountId: "private"`, baseline.tooling),
    "Worker public template must omit private account and routing values",
  );
  assert.equal(
    checkDeployTemplate(baseline.config.replace("REPLACE_VALUE", "literal"), baseline.tooling),
    "Worker public template vars must be placeholders",
  );
  assert.equal(
    checkDeployTemplate(baseline.config, "noBundle: false"),
    "Worker deploy tooling must upload the guarded bundle without rebundling",
  );
});
