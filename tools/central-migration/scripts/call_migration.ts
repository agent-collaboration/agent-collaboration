// PD7's migration calls against a deployed migration Worker (T2):
//
//   CENTRAL_OWNER_ACCESS_TOKEN=… node scripts/call_migration.ts inventory <origin> <mac-files.json> --out <inventory.json>
//   CENTRAL_OWNER_ACCESS_TOKEN=… node scripts/call_migration.ts migrate   <origin> <mac-files.json> --out <migrate.json>
//   CENTRAL_OWNER_ACCESS_TOKEN=… node scripts/call_migration.ts verify    <origin> --out <verify.json>
//   CENTRAL_OWNER_ACCESS_TOKEN=… node scripts/call_migration.ts revert    <origin> --out <revert.json>
//
// Every answer is written once, mode 0600, to --out (an existing file is
// refused). The token is never printed. Production's host needs --production.
// Exit 0 only when the step's answer is the success migration_call.ts defines.

import { readFile, writeFile } from "node:fs/promises";
import { parseExact, stringifyExact } from "../src/exact_json.ts";
import { callStep, type MigrationStep } from "./migration_call.ts";
import { PRODUCTION } from "./worker_deploy.ts";

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
const macFilesPath = rest.find(
  (argument, index) => !argument.startsWith("--") && rest[index - 1] !== "--out",
);
const token = process.env.CENTRAL_OWNER_ACCESS_TOKEN;
if (token === undefined || token === "") throw new Error("CENTRAL_OWNER_ACCESS_TOKEN is not set");
const url = new URL(origin);
if (url.protocol !== "https:") throw new Error("the origin must be https");
if (url.hostname === PRODUCTION.domain && !rest.includes("--production")) {
  throw new Error(`${url.hostname} is production; pass --production to call it`);
}
const macFiles = macFilesPath === undefined ? undefined : await readFile(macFilesPath, "utf8");

const result = await callStep(
  step as MigrationStep,
  async (method, path, body) => {
    const response = await fetch(new URL(path, origin), {
      method,
      headers: { "cf-access-token": token, "content-type": "application/json" },
      ...(body === undefined ? {} : { body }),
    });
    return { status: response.status, text: await response.text() };
  },
  macFiles,
  parseExact,
);
await writeFile(out, stringifyExact(result.answer), { mode: 0o600, flag: "wx" });
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
