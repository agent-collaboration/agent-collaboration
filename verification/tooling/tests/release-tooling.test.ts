import assert from "node:assert/strict";
import { chmodSync, readFileSync, mkdtempSync, writeFileSync, statSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { test } from "node:test";
import { z } from "zod";
import { compilerEnvironment } from "../sqlx-compiler-environment.ts";
import { countPhysicalLines } from "../check-rust-file-size.ts";
import { updateCodexRouterFormula, updateFormulaFile } from "../update-homebrew-formula.ts";
import {
  installAction,
  publishedAssetSha256,
  releaseAssetName,
  releaseSignatureProblem,
  validateInstallation,
  workspaceVersion,
} from "../publish-homebrew-tap-local.ts";
import type { CommandResult, CommandRunner } from "../process-command.ts";

const expectedSchema = z.union([z.object({ result: z.string() }), z.object({ error: z.string() })]);
const baselineSchema = z.object({
  sourceCommit: z.string(),
  formulaCases: z.array(
    z.object({
      formula: z.string(),
      release: z.object({ version: z.string(), sha256: z.string(), source_commit: z.string() }),
      expected: expectedSchema,
    }),
  ),
  wrappers: z.array(
    z.object({
      kind: z.enum(["package", "sqlite"]),
      environment: z.record(z.string(), z.string()),
      expected: z.record(z.string(), z.string()),
    }),
  ),
  lineCases: z.array(z.object({ hex: z.string(), expected: z.number() })),
  assetCases: z.array(
    z.object({ release: z.unknown(), name: z.string(), expected: expectedSchema }),
  ),
  installCases: z.array(
    z.object({ installed: z.string(), version: z.string(), expected: z.string() }),
  ),
});
const baselines = baselineSchema.parse(
  JSON.parse(readFileSync(new URL("fixtures/tooling-baselines.json", import.meta.url), "utf8")),
);

function observedResult(operation: () => string): z.infer<typeof expectedSchema> {
  try {
    return { result: operation() };
  } catch (error) {
    return { error: error instanceof Error ? error.message : String(error) };
  }
}

for (const [index, entry] of baselines.formulaCases.entries()) {
  test(`formula output matches the original release updater, case ${index}`, (): void => {
    const release = {
      version: entry.release.version,
      sha256: entry.release.sha256,
      sourceCommit: entry.release.source_commit,
    };
    assert.deepEqual(
      observedResult(() => updateCodexRouterFormula(entry.formula, release)),
      entry.expected,
    );
  });
}

for (const [index, entry] of baselines.wrappers.entries()) {
  test(`compiler environment matches original ${entry.kind} wrapper, case ${index}`, (): void => {
    assert.deepEqual(compilerEnvironment(entry.environment, entry.kind), entry.expected);
  });
}

for (const [index, entry] of baselines.lineCases.entries()) {
  test(`physical line count matches original byte input, case ${index}`, (): void => {
    assert.equal(countPhysicalLines(Buffer.from(entry.hex, "hex")), entry.expected);
  });
}

for (const [index, entry] of baselines.assetCases.entries()) {
  test(`release digest validation matches original, case ${index}`, (): void => {
    assert.deepEqual(
      observedResult(() => publishedAssetSha256(entry.release, entry.name)),
      entry.expected,
    );
  });
}

for (const [index, entry] of baselines.installCases.entries()) {
  test(`install action matches original, case ${index}`, (): void => {
    assert.equal(installAction(entry.installed, entry.version), entry.expected);
  });
}

test("formula writes preserve permissions and a repeated release is idempotent", (): void => {
  const directory = mkdtempSync(join(tmpdir(), "formula-write-"));
  try {
    const formulaPath = join(directory, "formula.rb");
    const original = readFileSync(new URL("fixtures/current-formula.rb", import.meta.url), "utf8");
    writeFileSync(formulaPath, original);
    chmodSync(formulaPath, 0o640);
    const release = { version: "0.1.103", sha256: "b".repeat(64), sourceCommit: "c".repeat(40) };
    assert.equal(updateFormulaFile(formulaPath, release), true);
    assert.equal(statSync(formulaPath).mode & 0o777, 0o640);
    assert.equal(updateFormulaFile(formulaPath, release), false);
    assert.equal(readFileSync(formulaPath, "utf8"), updateCodexRouterFormula(original, release));
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("workspace version ignores dependency versions and refuses non-workspace manifests", (): void => {
  assert.equal(
    workspaceVersion(
      '[package]\nversion = "9.9.9"\n[workspace.package]\nedition = "2024"\nversion = "0.1.102"\n[workspace.dependencies]\nversion_check = "9.9.9"\n',
    ),
    "0.1.102",
  );
  assert.throws(
    () => workspaceVersion('[package]\nversion = "1.0.0"\n'),
    /no \[workspace.package\] version/,
  );
  assert.equal(releaseAssetName("0.1.102"), "codex-router-v0.1.102-aarch64-apple-darwin.tar.gz");
});

test("release signatures require both the fixed release identifier and team", (): void => {
  assert.equal(
    releaseSignatureProblem(
      "Identifier=dev.shravansunder.agent-sessions\nTeamIdentifier=974QD84WVC\n",
      "agent-sessions",
    ),
    undefined,
  );
  for (const details of [
    "Identifier=agent_sessions-hash\nTeamIdentifier=not set\n",
    "Identifier=dev.shravansunder.agent-sessions.debug\nTeamIdentifier=974QD84WVC\n",
    "Identifier=dev.shravansunder.agent-sessions\nTeamIdentifier=AAAAAAAAAA\n",
  ])
    assert.notEqual(releaseSignatureProblem(details, "agent-sessions"), undefined);
});

test("installation validation checks the importer version as well as the other three executables", (): void => {
  for (const importerVersion of ["0.1.102", "0.1.101"]) {
    const runner: CommandRunner = (command): CommandResult => {
      let stdout = "";
      let stderr = "";
      const executable = command.at(-1)?.split("/").at(-1);
      if (command.join(" ") === "brew --prefix codex-router") stdout = "/isolated-install";
      else if (command[0] === "file") stdout = "Mach-O 64-bit executable arm64";
      else if (command[0] === "codesign" && command[1] === "-dv")
        stderr = `Identifier=dev.shravansunder.${executable}\nTeamIdentifier=974QD84WVC\n`;
      else if (command.at(-1) === "--version") {
        const name = command[0]?.split("/").at(-1);
        stdout = `${name} ${name === "codex-router-board-upgrade" ? importerVersion : "0.1.102"}`;
      }
      return { status: 0, stdout, stderr };
    };
    if (importerVersion === "0.1.102") validateInstallation("0.1.102", runner);
    else assert.throws(() => validateInstallation("0.1.102", runner), /codex-router-board-upgrade/);
  }
});
