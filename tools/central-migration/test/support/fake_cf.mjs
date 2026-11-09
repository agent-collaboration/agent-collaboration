#!/usr/bin/env node
// Test only: stands in for the cf CLI. It answers inventory commands from a
// JSON state file (FAKE_CF_STATE): `before` until a real deploy runs, then
// `after`. Every call is appended to the state's log, so a test can see exactly
// what ran, and the account each call was scoped to, and nothing is ever
// uploaded anywhere. FAKE_CF_VERSION changes the version it reports.
import { existsSync, readFileSync, writeFileSync } from "node:fs";

const path = process.env.FAKE_CF_STATE;
const state = JSON.parse(readFileSync(path, "utf8"));
const args = process.argv.slice(2);
state.log.push(args);
(state.accounts ??= []).push(process.env.CLOUDFLARE_ACCOUNT_ID ?? null);
const phase = state.deployed ? state.after : state.before;
let answer = "";
if (args[0] === "--version") {
  // Like the real cf, it hands over to a project-local cf inside a project.
  const local = existsSync("node_modules") ? "1.0.0-beta.17" : undefined;
  answer = `cf · v${local ?? process.env.FAKE_CF_VERSION ?? "0.11.0"}`;
} else if (args[0] === "deploy") {
  if (!args.includes("--dry-run")) state.deployed = true;
  answer = "deployed";
} else if (args.join(" ").startsWith("durable-objects namespaces list")) {
  answer = JSON.stringify(Number(args[args.indexOf("--page") + 1]) === 1 ? phase.namespaces : []);
} else if (args.join(" ").startsWith("workers scripts search")) {
  answer = JSON.stringify({ success: true, result: phase.workers });
} else if (args.join(" ").startsWith("workers domains list")) {
  answer = JSON.stringify(phase.domains);
} else {
  process.exitCode = 2;
}
writeFileSync(path, JSON.stringify(state));
process.stdout.write(answer);
