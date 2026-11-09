// PD7's migration calls against a deployed migration Worker (T2):
//
//   CENTRAL_OWNER_ACCESS_TOKEN=… node scripts/call_migration.ts inventory <origin> <mac-files.json> --out <inventory.json>
//   CENTRAL_OWNER_ACCESS_TOKEN=… node scripts/call_migration.ts migrate   <origin> <mac-files.json> --gate <gate-report.json> --out <migrate.json>
//   CENTRAL_OWNER_ACCESS_TOKEN=… node scripts/call_migration.ts verify    <origin> --out <verify.json>
//   CENTRAL_OWNER_ACCESS_TOKEN=… node scripts/call_migration.ts revert    <origin> --out <revert.json>
//
// --out is created exclusively, mode 0600, before the first request (an
// existing file refuses the run with nothing sent), and receives the answer
// once. No redirect is followed (owner_transport.ts). The token is never printed. Production's host needs --production.
// Exit 0 only when the step's answer is the success migration_call.ts defines.

import { readFile } from "node:fs/promises";
import { stringifyExact } from "../src/exact_json.ts";
import { migrateBody, type GateReport, type MigrationStep } from "./migration_call.ts";
import { callIntoFile, ownerTransport } from "./owner_transport.ts";
import { PRODUCTION, canonicalName } from "./worker_deploy.ts";

const [step, origin, ...rest] = process.argv.slice(2);
const steps: readonly string[] = ["inventory", "migrate", "verify", "revert"];
if (step === undefined || !steps.includes(step) || origin === undefined) {
  throw new Error(
    "usage: call_migration.ts <inventory|migrate|verify|revert> <origin> [mac-files.json] --out <file> [--production]",
  );
}
const outIndex = rest.indexOf("--out");
const out = outIndex < 0 ? undefined : rest[outIndex + 1];
if (out === undefined) throw new Error("--out <file> is required");
const flagValue = (flag: string): string | undefined => {
  const index = rest.indexOf(flag);
  return index < 0 ? undefined : rest[index + 1];
};
const macFilesPath = rest.find(
  (argument, index) =>
    !argument.startsWith("--") && rest[index - 1] !== "--out" && rest[index - 1] !== "--gate",
);
const token = process.env.CENTRAL_OWNER_ACCESS_TOKEN;
if (token === undefined || token === "") throw new Error("CENTRAL_OWNER_ACCESS_TOKEN is not set");
const url = new URL(origin);
if (url.protocol !== "https:") throw new Error("the origin must be https");
if (canonicalName(url.hostname) === PRODUCTION.domain && !rest.includes("--production")) {
  throw new Error(`${url.hostname} is production; pass --production to call it`);
}
const macFiles = macFilesPath === undefined ? undefined : await readFile(macFilesPath, "utf8");
const gatePath = flagValue("--gate");
if (step === "migrate" && (gatePath === undefined || macFiles === undefined)) {
  throw new Error("migrate needs <mac-files.json> and --gate <the decode gate's pass report>");
}
const body =
  step === "migrate" && macFiles !== undefined && gatePath !== undefined
    ? migrateBody(
        macFiles,
        JSON.parse(await readFile(gatePath, "utf8")) as GateReport,
        stringifyExact,
      )
    : macFiles;

const result = await callIntoFile(out, step as MigrationStep, ownerTransport(origin, token), body);
console.log(
  JSON.stringify({
    step: result.step,
    status: result.status,
    success: result.success,
    reason: result.reason,
    answer: out,
  }),
);
if (!result.success) process.exitCode = 1;
