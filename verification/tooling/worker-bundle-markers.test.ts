import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { z } from "zod";
import { checkWorkerBundle } from "./check-central-worker-bundle.ts";

const baseline = z
  .object({
    cargo: z.string(),
    readme: z.string(),
    config: z.string(),
    tooling: z.string(),
    cases: z.array(
      z.object({
        mode: z.string(),
        content: z.string(),
        expected: z.object({ status: z.number() }),
      }),
    ),
  })
  .parse(
    JSON.parse(
      readFileSync(new URL("./fixtures/worker-bundle-baselines.json", import.meta.url), "utf8"),
    ),
  );
const oldFaults = baseline.cases.find(
  (entry): boolean => entry.mode === "faults" && entry.expected.status === 0,
);
if (oldFaults === undefined) throw new Error("faults corpus absent");
const fleetRoutes = ["/admin/v1/conformance/fleet-schema", "/test/fleet-schema"];
const faults = [...new Set([...oldFaults.content.split("\n"), ...fleetRoutes])];

function check(mode: "production" | "faults", content: string): number {
  const root = mkdtempSync(join(tmpdir(), "d1-worker-route-"));
  try {
    const worker = join(root, "crates/collaboration-central-cloudflare");
    mkdirSync(join(worker, "build"), { recursive: true });
    writeFileSync(join(root, "Cargo.toml"), baseline.cargo);
    writeFileSync(join(worker, "README.md"), baseline.readme);
    writeFileSync(join(worker, "cloudflare.config.ts"), baseline.config);
    writeFileSync(join(worker, "wrangler.config.ts"), baseline.tooling);
    writeFileSync(join(worker, "build/index.js"), content);
    writeFileSync(join(worker, "build/index_bg.wasm"), "");
    return checkWorkerBundle(mode, root).status;
  } finally {
    rmSync(root, { recursive: true });
  }
}

for (const route of fleetRoutes) {
  test(`production rejects the new conformance route ${route}`, (): void => {
    assert.equal(check("production", route), 1);
  });
  test(`faults require the new conformance route ${route}`, (): void => {
    assert.equal(
      check("faults", faults.filter((marker): boolean => marker !== route).join("\n")),
      1,
    );
  });
}
