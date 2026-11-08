import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { z } from "zod";
import {
  checkDeployTemplate,
  checkLinkTemplate,
  checkWorkerBundle,
  linkEntryMarker,
  linkPrebuild,
  publicEntryMarker,
} from "./check-central-worker-bundle.ts";

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

const linkConfig = readFileSync(
  new URL(
    "../../crates/collaboration-central-cloudflare/cloudflare-link.config.ts",
    import.meta.url,
  ),
  "utf8",
);

function guardRoot(publicBundle: string, linkBundle: string): string {
  const root = mkdtempSync(join(tmpdir(), "worker-link-guard-"));
  const worker = join(root, "crates/collaboration-central-cloudflare");
  for (const directory of ["build", "build-link"])
    mkdirSync(join(worker, directory), { recursive: true });
  writeFileSync(join(root, "Cargo.toml"), baseline.cargo);
  writeFileSync(join(worker, "README.md"), `${baseline.readme}\n${linkPrebuild}\n`);
  writeFileSync(join(worker, "cloudflare.config.ts"), baseline.config);
  writeFileSync(join(worker, "cloudflare-link.config.ts"), linkConfig);
  writeFileSync(join(worker, "wrangler.config.ts"), baseline.tooling);
  writeFileSync(join(worker, "build/index.js"), publicBundle);
  writeFileSync(join(worker, "build/index_bg.wasm"), "");
  writeFileSync(join(worker, "build-link/index.js"), linkBundle);
  writeFileSync(join(worker, "build-link/index_bg.wasm"), "");
  return root;
}

test("each Worker artifact passes only as itself", (): void => {
  // Wrong builds caught: central-link deployed as public central, or public
  // central deployed where the link belongs.
  const publicBundle = `export default {}; // ${publicEntryMarker}`;
  const linkBundle = `export default {}; // ${linkEntryMarker}`;
  for (const [publicArtifact, linkArtifact, expected] of [
    [publicBundle, linkBundle, [0, 0]],
    [linkBundle, publicBundle, [1, 1]],
  ] as const) {
    const root = guardRoot(publicArtifact, linkArtifact);
    try {
      assert.deepEqual(
        [checkWorkerBundle("production", root).status, checkWorkerBundle("link", root).status],
        expected,
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("the link template refuses any public surface or Durable Object class", (): void => {
  assert.equal(checkLinkTemplate(linkConfig, baseline.tooling), undefined);
  for (const [edited, message] of [
    [
      linkConfig.replace("workersDev: false", "workersDev: true"),
      "central-link template must set workersDev: false",
    ],
    [
      linkConfig.replace("previewUrls: false", "previewUrls: true"),
      "central-link template must set previewUrls: false",
    ],
    [
      `${linkConfig}, routes: []`,
      "central-link template must omit private account and routing values",
    ],
    [
      linkConfig.replace("env: {", "exports: {}, env: {"),
      "central-link must declare no Durable Object class or Copy binding",
    ],
    [
      linkConfig.replace("replace-central-worker-name", "replace-link-worker-name"),
      "central-link must bind central's existing FleetObject class by central's script name",
    ],
    [
      linkConfig.replace("build-link/index.js", "build/index.js"),
      "central-link template must upload the link artifact",
    ],
  ] as const)
    assert.equal(checkLinkTemplate(edited, baseline.tooling), message);
});
