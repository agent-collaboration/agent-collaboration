import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { z } from "zod";
import { checkDeployTemplate, checkWorkerBundle } from "./check-central-worker-bundle.ts";
import { testSupportMarkers } from "./worker-test-support-markers.ts";

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

test("a production bundle carrying any storage test-support marker is refused", (): void => {
  for (const marker of testSupportMarkers) {
    const root = mkdtempSync(join(tmpdir(), "worker-bundle-guard-"));
    try {
      const worker = join(root, "crates/collaboration-central-cloudflare");
      mkdirSync(join(worker, "build"), { recursive: true });
      writeFileSync(join(root, "Cargo.toml"), baseline.cargo);
      writeFileSync(join(worker, "README.md"), baseline.readme);
      writeFileSync(join(worker, "cloudflare.config.ts"), baseline.config);
      writeFileSync(join(worker, "wrangler.config.ts"), baseline.tooling);
      writeFileSync(join(worker, "build/index.js"), `route(${JSON.stringify(marker)})`);
      writeFileSync(join(worker, "build/index_bg.wasm"), "");
      assert.deepEqual(checkWorkerBundle("production", root), {
        status: 1,
        message: "Worker production bundle contains the storage test-support routes",
      });
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

function selectionFixture(): { root: string; defaultDirectory: string; selectedDirectory: string } {
  const root = mkdtempSync(join(tmpdir(), "worker-bundle-selection-"));
  const worker = join(root, "crates/collaboration-central-cloudflare");
  const defaultDirectory = join(worker, "build");
  const selectedDirectory = join(worker, "build-link");
  mkdirSync(defaultDirectory, { recursive: true });
  mkdirSync(selectedDirectory);
  writeFileSync(join(root, "Cargo.toml"), baseline.cargo);
  writeFileSync(join(worker, "README.md"), baseline.readme);
  writeFileSync(join(worker, "cloudflare.config.ts"), baseline.config);
  writeFileSync(join(worker, "wrangler.config.ts"), baseline.tooling);
  for (const directory of [defaultDirectory, selectedDirectory]) {
    writeFileSync(join(directory, "index.js"), "// clean production fixture");
    writeFileSync(join(directory, "index_bg.wasm"), "");
  }
  return { root, defaultDirectory, selectedDirectory };
}

test("an explicit artifact directory inspects selected bytes and never falls back", (): void => {
  const { root, defaultDirectory, selectedDirectory } = selectionFixture();
  try {
    writeFileSync(join(defaultDirectory, "index.js"), 'route("/test-support/workspace/")');
    assert.equal(checkWorkerBundle("production", root).status, 1);
    assert.equal(checkWorkerBundle("production", root, selectedDirectory).status, 0);
    writeFileSync(join(defaultDirectory, "index.js"), "// clean production fixture");
    writeFileSync(join(selectedDirectory, "index_bg.wasm"), "/test/fail-alarm-batch");
    assert.deepEqual(checkWorkerBundle("production", root, selectedDirectory), {
      status: 1,
      message: "Worker production bundle contains the storage test-support routes",
    });
    rmSync(selectedDirectory, { recursive: true });
    assert.deepEqual(checkWorkerBundle("production", root, selectedDirectory), {
      status: 1,
      message: "Worker bundle missing; build it before checking",
    });
    assert.equal(checkWorkerBundle("production", root).status, 0);
  } finally {
    rmSync(root, { recursive: true });
  }
});

test("bundle-directory CLI selects actual alternate artifacts and preserves the default command", (): void => {
  const { root, defaultDirectory, selectedDirectory } = selectionFixture();
  const script = fileURLToPath(new URL("./check-central-worker-bundle.ts", import.meta.url));
  const invoke = (arguments_: readonly string[]): ReturnType<typeof spawnSync> =>
    spawnSync(process.execPath, [script, ...arguments_], { cwd: root, encoding: "utf8" });
  try {
    writeFileSync(join(defaultDirectory, "index.js"), 'route("/test-support/workspace/")');
    assert.equal(invoke(["production"]).status, 1);
    const selected = invoke([
      "production",
      "--bundle-directory",
      "crates/collaboration-central-cloudflare/build-link",
    ]);
    assert.equal(selected.status, 0, String(selected.stderr));
    assert.match(String(selected.stdout), /Worker production bundle: conformance-control/);
    writeFileSync(join(defaultDirectory, "index.js"), "// clean production fixture");
    writeFileSync(join(selectedDirectory, "index.js"), 'route("/test/fail-alarm-batch")');
    assert.equal(invoke(["production", "--bundle-directory", selectedDirectory]).status, 1);
    rmSync(join(selectedDirectory, "index_bg.wasm"));
    const missing = invoke(["production", "--bundle-directory", selectedDirectory]);
    assert.equal(missing.status, 1);
    assert.match(String(missing.stdout), /Worker bundle missing/);
    assert.equal(invoke(["production"]).status, 0);
    for (const args of [
      [],
      ["production", "--bundle-directory"],
      ["production", "--other", selectedDirectory],
      ["production", "--bundle-directory", ""],
      ["production", "--bundle-directory", selectedDirectory, "extra"],
    ])
      assert.equal(invoke(args).status, 2);
  } finally {
    rmSync(root, { recursive: true });
  }
});
