import assert from "node:assert/strict";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  symlinkSync,
} from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { test } from "node:test";
import { z } from "zod";
import { declaredProductionModules, stripCfgTestItems } from "./rust-production-sources.ts";
import { functionBody } from "./check-http-streaming-guard.ts";
import {
  isUnderPath,
  sanitized,
  selfTestSchema,
  validationChecks,
} from "./validate-proxy-db-runtime-isolation-artifact.ts";

const sourceCases = z
  .array(
    z.object({
      source: z.string(),
      relativePath: z.string(),
      stripped: z.string(),
      modules: z.array(z.string()),
    }),
  )
  .parse(
    JSON.parse(
      readFileSync(new URL("./fixtures/rust-source-baselines.json", import.meta.url), "utf8"),
    ),
  );
for (const [index, entry] of sourceCases.entries()) {
  test(`Rust production discovery and test stripping match original source input ${index}`, (): void => {
    assert.equal(stripCfgTestItems(entry.source), entry.stripped);
    assert.deepEqual(declaredProductionModules(entry.relativePath, entry.source), entry.modules);
  });
}

test("HTTP streaming guard finds complete nested function bodies and rejects missing bodies", (): void => {
  assert.equal(
    functionBody("\nasync fn guarded() { if flag { preserve(); } }", "guarded"),
    "{ if flag { preserve(); } }",
  );
  assert.throws(() => functionBody("", "missing"), /missing function missing/);
  assert.throws(() => functionBody("\nfn missing()", "missing"), /missing function body/);
  assert.throws(() => functionBody("\nfn missing() {", "missing"), /unterminated function body/);
});

const artifactCases = new URL("./proxy-db-artifact-cases/", import.meta.url);
for (const name of readdirSync(artifactCases).sort()) {
  const entry = selfTestSchema.parse(
    JSON.parse(readFileSync(new URL(name, artifactCases), "utf8")),
  );
  test(`proxy DB artifact validation matches original ${entry.name}`, (): void => {
    const root = realpathSync(process.cwd());
    const checks = validationChecks(entry.payload, {
      artifactBasename: entry.artifact,
      repositoryRoot: root,
      routerRoot: entry.router.replace("<root>", root),
      codexHome: entry.codex.replace("<root>", root),
      processHome: entry.home.replace("<root>", root),
      thresholdMs: entry.threshold,
    });
    assert.deepEqual(
      checks.filter((check) => !check.passed).map((check) => check.name),
      entry.expected,
    );
  });
}

test("artifact paths follow symlinks even when their declared tails do not exist", (): void => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "artifact-path-")));
  try {
    mkdirSync(join(root, "repo"));
    mkdirSync(join(root, "outside"));
    symlinkSync(join(root, "outside/missing"), join(root, "repo/link"));
    assert.equal(isUnderPath(join(root, "repo/link/tail"), join(root, "repo")), false);
    assert.equal(sanitized(join(root, "repo/link/tail"), join(root, "repo")), "<external>");
    assert.equal(sanitized(join(root, "repo/missing"), join(root, "repo")), "<repo>/missing");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
