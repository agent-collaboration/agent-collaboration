import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, realpath, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { declarationSchema } from "./declaration.ts";
import { importDeclarationSchema } from "./import-declaration.ts";
import { prepareSyntheticLegacy } from "./inputs/synthetic-legacy.ts";
import { Receipt } from "./receipt.ts";
import { collectOutcomes, type CaseOutcome, type CaseRegistration } from "./coverage.ts";
import { forwardedPostsCase } from "./cases/forwarded-posts.ts";

const repository = fileURLToPath(new URL("..", import.meta.url)).replace(/\/$/, "");
const sourcePin = execFileSync("git", ["rev-parse", "HEAD"], {
  cwd: repository,
  encoding: "utf8",
}).trim();
const sha256 = "0".repeat(64);
const artifact = {
  path: "/private/tmp/test-artifact",
  sha256,
  identifier: "test-only",
  team: "test-only",
};
const source = {
  repository,
  productPin: sourcePin,
  harnessPin: sourcePin,
  productVersion: "0.1.102",
  purpose: "discovery",
  importer: artifact,
  evidenceDirectory: "/private/tmp/test-evidence",
  vd: { repository, revision: sourcePin, path: "docs/specs/test/verification-design.md" },
  deadline: "2099-01-01T00:00:00Z",
};
const local = {
  kind: "local",
  name: "mac",
  root: "/private/tmp/test-run/mac",
  temporaryParent: "/private/tmp/test-run",
  nativeCwd: "/private/tmp",
  codexHome: "/private/tmp/test-codex",
  port: 44001,
  router: artifact,
  cli: artifact,
  enrollment: { kind: "fixture" },
};
const remote = {
  ...local,
  kind: "ssh",
  name: "sunclaw",
  host: "synthetic.invalid",
  port: 44002,
  root: "/private/tmp/test-run/remote",
  retainedRoots: ["/private/tmp/test-run/remote"],
};
const target = {
  kind: "tokio",
  root: "/private/tmp/test-central",
  assertionFile: "/private/tmp/test-assertion",
  fixtureKey: { path: "/private/tmp/test-key", sha256 },
  binary: artifact,
  projects: ["019a0000-0000-7000-8000-000000000015"],
};
const synthetic = {
  ...source,
  dataBoundary: "syntheticOnly",
  syntheticFixture: {
    path: repository + "/crates/board-legacy-upgrade/tests/fixtures/role-history.sqlite",
    sha256,
  },
  target,
  machines: [local, remote],
  recipient: { model: "gpt-6-luna", provider: "openai", effort: "low" },
  rung: "R1",
  rows: ["OB4:F"],
};
const real = {
  ...source,
  dataBoundary: "realCopiesMacOnly",
  machine: local,
  target,
  legacyStores: [
    {
      path: "/private/tmp/sealed/source.sqlite",
      copyRoot: "/private/tmp/sealed",
      sha256,
      oracle: artifact,
    },
  ],
};

test("cross-machine declarations reject real sealed inputs and the retired combined shape", () => {
  assert.equal(declarationSchema.safeParse(synthetic).success, true);
  assert.equal(
    declarationSchema.safeParse({ ...synthetic, legacyStores: real.legacyStores }).success,
    false,
  );
  assert.equal(
    declarationSchema.safeParse({ ...synthetic, dataBoundary: "realCopiesMacOnly" }).success,
    false,
  );
  const { dataBoundary: _boundary, syntheticFixture: _fixture, ...retired } = synthetic;
  assert.equal(
    declarationSchema.safeParse({ ...retired, legacyStores: real.legacyStores }).success,
    false,
  );
});
test("real-copy declarations reject remote machines and native recipient configuration", () => {
  assert.equal(importDeclarationSchema.safeParse(real).success, true);
  for (const invalid of [
    { ...real, machine: remote },
    { ...real, recipient: synthetic.recipient },
    { ...real, machines: synthetic.machines },
    { ...real, dataBoundary: "syntheticOnly" },
  ])
    assert.equal(importDeclarationSchema.safeParse(invalid).success, false);
});
test("cross-machine fixture preparation refuses an arbitrary source before opening it", async () => {
  const declaration = declarationSchema.parse({
    ...synthetic,
    syntheticFixture: { path: "/never-read-real-source.sqlite", sha256 },
  });
  await assert.rejects(
    prepareSyntheticLegacy(declaration, new Receipt("/never-created-evidence")),
    /committed synthetic/,
  );
});
test("cross-machine inputs are sealed independent copies of the pinned synthetic fixture", async () => {
  const root = await realpath(await mkdtemp(join(tmpdir(), "2026-10-verification-boundary-")));
  try {
    const fixture = await readFile(synthetic.syntheticFixture.path);
    const digest = createHash("sha256").update(fixture).digest("hex");
    const declaration = declarationSchema.parse({
      ...synthetic,
      evidenceDirectory: root + "/evidence",
      syntheticFixture: { ...synthetic.syntheticFixture, sha256: digest },
    });
    const receipt = new Receipt(declaration.evidenceDirectory);
    await receipt.begin(declaration);
    const copy = await prepareSyntheticLegacy(declaration, receipt);
    assert.equal(copy.input.sha256, digest);
    assert.notEqual(copy.input.path, declaration.syntheticFixture.path);
    assert.deepEqual(await readFile(copy.input.path), fixture);
    assert.equal((await stat(copy.input.path)).mode & 0o777, 0o400);
    await copy.verify();
  } finally {
    await rm(root, { recursive: true });
  }
});
test("F delivery alone cannot close non-retention and component failures remain failures", () => {
  const rows = new Map([["OB4:F", { id: "OB4:F", reference: "VD OB4 F", behavior: "forwarding" }]]);
  const cases: CaseRegistration[] = [
    forwardedPostsCase,
    { id: "synthetic-native-evidence", rows: ["OB4:F"], evidence: "tokioComponent" },
    { id: "synthetic-worker-evidence", rows: ["OB4:F"], evidence: "workerdComponent" },
  ];
  const results = new Map<string, CaseOutcome>([[forwardedPostsCase.id, { kind: "observed" }]]);
  const collect = (registered: readonly CaseRegistration[]) =>
    collectOutcomes({ rows, selected: ["OB4:F"], cases: registered, results }).get("OB4:F");
  assert.equal(collect([forwardedPostsCase])?.kind, "open");
  for (const component of cases.slice(1)) results.set(component.id, { kind: "observed" });
  assert.equal(collect(cases)?.kind, "observed");
  results.set("synthetic-worker-evidence", {
    kind: "failed",
    reason: "retainer control not restored",
  });
  assert.equal(collect(cases)?.kind, "failed");
});
