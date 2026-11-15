// Drives only the declared, prebuilt, signed Rust socket fixture. No build,
// signing, token creation, deployment or production runtime is performed here.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { closeSync, openSync, readFileSync, realpathSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { basename, isAbsolute, join } from "node:path";
import { z } from "zod";
import { assertC2ProductReceipt } from "./c2-product-receipt.ts";

export const c2ProductSelector =
  "workspace_requests::machine_link::hibernating_socket_restores_live_authority_and_semantic_store";
const digest = z.string().regex(/^[0-9a-f]{64}$/);
const bundle = z.object({ indexJsSha256: digest, wasmSha256: digest }).strict();
export const c2ProductPlanSchema = z
  .object({
    repository: z.string().refine(isAbsolute),
    sourceHead: z.string().regex(/^[0-9a-f]{40}$/),
    evidence: z.string().refine(isAbsolute),
    binary: z.object({ path: z.string().refine(isAbsolute), sha256: digest }).strict(),
    publicBundle: bundle,
    privateBundle: bundle,
  })
  .strict();
type C2ProductPlan = z.infer<typeof c2ProductPlanSchema>;

function sha256(path: string): string {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}
function commandText(command: string, arguments_: readonly string[], cwd: string): string {
  const result = spawnSync(command, arguments_, { cwd, encoding: "utf8" });
  if (result.error !== undefined) throw result.error;
  assert.equal(result.status, 0, `${command} preflight failed`);
  return result.stdout.trim();
}
function frozenBinding(plan: C2ProductPlan): Readonly<Record<string, string>> {
  assert.equal(realpathSync(plan.repository), plan.repository, "repository must be canonical");
  assert.equal(
    commandText("git", ["rev-parse", "HEAD"], plan.repository),
    plan.sourceHead,
    "frozen source head mismatch",
  );
  assert.equal(
    commandText("git", ["status", "--porcelain", "--untracked-files=no"], plan.repository),
    "",
    "frozen tracked sources must be clean",
  );
  assert.equal(
    commandText(
      "git",
      ["ls-files", "--others", "--exclude-standard", "--", "crates", "verification", "scripts"],
      plan.repository,
    ),
    "",
    "frozen target must not contain untracked runnable sources",
  );
  assert.match(
    basename(plan.binary.path),
    /^integration-[0-9a-f]{16}$/,
    "declare the integration test executable",
  );
  assert.equal(
    realpathSync(plan.binary.path),
    join(plan.repository, "target/debug/deps", basename(plan.binary.path)),
  );
  const hashes: Record<string, string> = { [plan.binary.path]: sha256(plan.binary.path) };
  assert.equal(hashes[plan.binary.path], plan.binary.sha256, "declared test binary hash mismatch");
  for (const [directory, expected] of [
    ["build-test-support", plan.publicBundle],
    ["build-link-test-support", plan.privateBundle],
  ] as const) {
    for (const [name, wanted] of [
      ["index.js", expected.indexJsSha256],
      ["index_bg.wasm", expected.wasmSha256],
    ] as const) {
      const path = join(
        plan.repository,
        "crates/collaboration-central-cloudflare",
        directory,
        name,
      );
      hashes[path] = sha256(path);
      assert.equal(hashes[path], wanted, `declared ${directory}/${name} hash mismatch`);
    }
  }
  return hashes;
}
function requireSignedBinary(plan: C2ProductPlan): void {
  assert.equal(
    process.platform,
    "darwin",
    "this declared-target driver requires the Mac signing contract",
  );
  commandText("/usr/bin/codesign", ["--verify", "--strict", plan.binary.path], plan.repository);
  const signature = spawnSync("/usr/bin/codesign", ["-dv", "--verbose=4", plan.binary.path], {
    encoding: "utf8",
  });
  assert.equal(signature.status, 0, "signature details unavailable");
  const fields = signature.stderr.split("\n");
  assert.ok(
    fields.includes("Identifier=dev.shravansunder.test.integration") &&
      fields.includes("TeamIdentifier=974QD84WVC") &&
      fields.some((field) => field.startsWith("Authority=Developer ID Application:")),
    "test code identity mismatch",
  );
}
export function requireSingleC2Selection(listing: string): void {
  assert.deepEqual(
    listing.trim().split(/\r?\n/),
    [`${c2ProductSelector}: test`],
    "exact C2 selector must discover one nonempty test",
  );
}
async function executeFixture(plan: C2ProductPlan, receipt: string): Promise<number | null> {
  const output = openSync(join(plan.evidence, "fixture.stdout.log"), "wx", 0o600);
  let errors: number | undefined;
  try {
    errors = openSync(join(plan.evidence, "fixture.stderr.log"), "wx", 0o600);
    const child = spawn(
      plan.binary.path,
      ["--exact", c2ProductSelector, "--nocapture", "--test-threads=1"],
      {
        cwd: plan.repository,
        env: { ...process.env, CODEX_ROUTER_C2_HIBERNATION_RECEIPT: receipt },
        stdio: ["ignore", output, errors],
      },
    );
    // Own settlement before any awaited file I/O: a short-lived child can exit
    // before its process record is written.
    const settled = new Promise<
      { kind: "closed"; code: number | null } | { kind: "failed"; error: Error }
    >((resolve): void => {
      child.once("error", (error): void => resolve({ kind: "failed", error }));
      child.once("close", (code): void => resolve({ kind: "closed", code }));
    });
    let interrupted = false;
    const interrupt = (): void => {
      interrupted = true;
      if (child.exitCode === null) child.kill("SIGINT");
    };
    const terminate = (): void => {
      interrupted = true;
      if (child.exitCode === null) child.kill("SIGTERM");
    };
    process.on("SIGINT", interrupt);
    process.on("SIGTERM", terminate);
    try {
      await writeFile(
        join(plan.evidence, "fixture-process.json"),
        JSON.stringify(
          { pid: child.pid, executable: plan.binary.path, selector: c2ProductSelector },
          null,
          2,
        ) + "\n",
        { mode: 0o600 },
      );
      const settlement = await settled;
      if (settlement.kind === "failed") throw settlement.error;
      assert.equal(interrupted, false, "interrupted observation cannot pass");
      return settlement.code;
    } catch (error: unknown) {
      if (child.exitCode === null) child.kill("SIGTERM");
      await settled;
      throw error;
    } finally {
      process.removeListener("SIGINT", interrupt);
      process.removeListener("SIGTERM", terminate);
    }
  } finally {
    closeSync(output);
    if (errors !== undefined) closeSync(errors);
  }
}

export async function runC2ProductPlan(path: string): Promise<void> {
  const plan = c2ProductPlanSchema.parse(JSON.parse(await readFile(path, "utf8")));
  await mkdir(plan.evidence, { mode: 0o700 }); // Fresh evidence; never overwrite an earlier red.
  await writeFile(join(plan.evidence, "declared-plan.json"), JSON.stringify(plan, null, 2) + "\n", {
    mode: 0o600,
  });
  try {
    await observeC2Product(plan);
  } catch (error: unknown) {
    await writeFile(
      join(plan.evidence, "failure.json"),
      JSON.stringify(
        {
          kind: "c2ObservationFailed",
          message: error instanceof Error ? error.message : "unknown failure",
          productCoverage: "unverified",
        },
        null,
        2,
      ) + "\n",
      { mode: 0o600 },
    );
    throw error;
  }
}

async function observeC2Product(plan: C2ProductPlan): Promise<void> {
  const before = frozenBinding(plan);
  requireSignedBinary(plan);
  const listing = commandText(
    plan.binary.path,
    ["--list", "--format", "terse", "--exact", c2ProductSelector],
    plan.repository,
  );
  requireSingleC2Selection(listing);
  const binding = {
    plan,
    before,
    node: process.versions.node,
    libuv: process.versions.uv,
    driverSha256: sha256(new URL(import.meta.url).pathname),
    receiptCheckerSha256: sha256(new URL("./c2-product-receipt.ts", import.meta.url).pathname),
    observerSha256: sha256(new URL("./c2-witness.ts", import.meta.url).pathname),
  };
  await writeFile(
    join(plan.evidence, "binding-before.json"),
    JSON.stringify(binding, null, 2) + "\n",
    { mode: 0o600 },
  );
  const receiptPath = join(plan.evidence, "product-receipt.json");
  const exitCode = await executeFixture(plan, receiptPath);
  const after = frozenBinding(plan);
  assert.deepEqual(after, before, "source/binary/bundle hold changed during observation");
  await writeFile(
    join(plan.evidence, "binding-after.json"),
    JSON.stringify(after, null, 2) + "\n",
    { mode: 0o600 },
  );
  assert.equal(exitCode, 0, "actual Rust socket fixture failed; preserve its raw logs");
  const output = await readFile(join(plan.evidence, "fixture.stdout.log"), "utf8");
  assert.match(
    output,
    /test result: ok\. 1 passed; 0 failed; 0 ignored;/,
    "one actual test must pass without ignores",
  );
  const receipt = assertC2ProductReceipt(JSON.parse(await readFile(receiptPath, "utf8")));
  const result = {
    kind: "c2PositiveObserved",
    selector: c2ProductSelector,
    sourceHead: plan.sourceHead,
    exitCode,
    disposed: receipt.disposed,
    area: receipt.area,
    compiledNegative: "unverified-pending-bounded-probe",
    naturalIdle: "unverified",
    hosted: "unverified",
  };
  await writeFile(join(plan.evidence, "observation.json"), JSON.stringify(result, null, 2) + "\n", {
    mode: 0o600,
  });
  process.stdout.write(`${JSON.stringify(result)}\n`);
}
