import { lstat, readFile } from "node:fs/promises";
import { declarationSchema } from "./declaration.ts";
import { encode } from "./exact-json.ts";
import {
  collectOutcomes,
  exitCode,
  readRows,
  resolveCoverage,
  type CaseRegistration,
} from "./coverage.ts";
import { casesFor } from "./case-registry.ts";
import { Receipt } from "./receipt.ts";

async function main(): Promise<void> {
  const path = process.argv[2];
  if (path === undefined || process.argv.length !== 3)
    throw new Error("usage: node verification/coverage-plan.ts PRIVATE_DECLARATION.json");
  const info = await lstat(path);
  if (
    !info.isFile() ||
    info.isSymbolicLink() ||
    info.uid !== process.getuid?.() ||
    (info.mode & 0o077) !== 0
  )
    throw new Error("declaration must be an owner-private regular file");
  const declaration = declarationSchema.parse(JSON.parse(await readFile(path, "utf8")));
  const registeredCases: readonly CaseRegistration[] = casesFor();
  const receipt = new Receipt(declaration.evidenceDirectory);
  await receipt.begin(declaration);
  const rows = readRows(declaration.vd);
  const mapping = resolveCoverage(rows, declaration.rows, registeredCases);
  const outcomes = collectOutcomes({
    rows,
    selected: declaration.rows,
    cases: registeredCases,
    results: new Map(),
  });
  const missing = [...mapping]
    .filter(([, cases]): boolean => cases.length === 0)
    .map(([row]): string => row);
  await receipt.save("coverage-plan.json", {
    mode: "planOnly",
    executedCases: 0,
    declarationFrozenBeforeEffects: true,
    targetStarted: false,
    credentialsResolved: false,
    selected: declaration.rows,
    mapping: Object.fromEntries(mapping),
    missing,
    outcomes: Object.fromEntries(outcomes),
    exit: exitCode(outcomes),
  });
  process.stdout.write(
    `${encode({
      mode: "planOnly",
      selectedRows: declaration.rows.length,
      registeredCases: registeredCases.length,
      unmappedRows: missing.length,
      executedCases: 0,
      exit: exitCode(outcomes),
    })}\n`,
  );
  process.exitCode = exitCode(outcomes);
}
await main();
