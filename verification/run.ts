import { readFile } from "node:fs/promises";
import { encode } from "./exact-json.ts";
import { crossMachineUnavailable } from "./run-setup.ts";
import { assessD1Coverage, d1Observations } from "./observations/central/d1-coverage.ts";
import { readD1HarnessDeclaration, runD1Harness } from "./observations/central/d1-harness.ts";
import { d1ReadbackSchema, readD1Stores } from "./observations/central/d1-sqlite-readback.ts";

function printPreparation(): boolean {
  if (process.argv.length === 3 && process.argv[2] === "--help") {
    process.stdout.write(
      "node verification/run.ts PRIVATE_DECLARATION.json\n" +
        `${crossMachineUnavailable}\n` +
        "node verification/run.ts --d1-plan\n" +
        "Print D1 O1–O11 requirements and all-open coverage. No launch, network, credentials or writes.\n" +
        "node verification/run.ts --d1-harness PRIVATE_DECLARATION.json\n" +
        "Drive three isolated native-backed Hosts and selected no-model fixtures on local Tokio.\n" +
        "node verification/run.ts --d1-readback PRIVATE_DECLARATION.json\n" +
        "Read exact stopped task stores after consumer checks.\n" +
        "D1 full outcome credit requires declared controls/target; staging is never executed here.\n",
    );
    return true;
  }
  if (process.argv.length === 3 && process.argv[2] === "--d1-plan") {
    process.stdout.write(
      `${encode({
        suite: "d1-central-layout-and-mail",
        verificationDesign:
          "docs/specs/2026-11-02-central-storage-reset/verification-design/d1-central-layout-and-mail.md",
        executed: false,
        productProofEligible: false,
        observations: d1Observations,
        outcomes: Object.fromEntries(assessD1Coverage({ backend: "tokio", checks: new Map() })),
      })}\n`,
    );
    return true;
  }
  return false;
}
async function runD1Action(): Promise<boolean> {
  if (process.argv.length === 4 && process.argv[2] === "--d1-readback") {
    const path = process.argv[3];
    if (path === undefined) throw new Error("D1 readback declaration absent");
    await readD1Stores(d1ReadbackSchema.parse(JSON.parse(await readFile(path, "utf8"))));
    return true;
  }
  if (process.argv.length === 4 && process.argv[2] === "--d1-harness") {
    const path = process.argv[3];
    if (path === undefined) throw new Error("D1 declaration path absent");
    await runD1Harness(await readD1HarnessDeclaration(path));
    return true;
  }
  return false;
}
async function main(): Promise<void> {
  if (await runD1Action()) return;
  if (printPreparation()) return;
  process.stderr.write(`${crossMachineUnavailable}\n`);
  process.exitCode = 2;
}
try {
  await main();
} catch {
  process.stderr.write(
    "Verification stopped with unconfirmed inputs or evidence; no automatic replay.\n",
  );
  process.exitCode = 2;
}
