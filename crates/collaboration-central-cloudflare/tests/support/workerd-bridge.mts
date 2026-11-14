// Starts the established loader. Requests and business assertions belong to Rust.
import { createInterface } from "node:readline";
import { access, readFile } from "node:fs/promises";
import { join } from "node:path";
import {
  WorkerdStorage,
  centralTestBundle,
  centralLinkTestBundle,
  workerdSignInPlanSchema,
} from "../../../../verification/targets/workerd-storage.ts";
const arguments_ = process.argv.slice(2);
const [option, planPath] = arguments_;
if (
  arguments_.length !== 0 &&
  (arguments_.length !== 2 || option !== "--signin-plan" || planPath === undefined)
)
  throw new Error("usage: workerd-bridge.mts [--signin-plan <fixture-plan.json>]");
const plan =
  planPath === undefined
    ? undefined
    : workerdSignInPlanSchema.parse(JSON.parse(await readFile(planPath, "utf8")));
const bundle = centralTestBundle();
const linkBundle = plan === undefined ? undefined : centralLinkTestBundle();
for (const selected of linkBundle === undefined ? [bundle] : [bundle, linkBundle]) {
  for (const name of ["index.js", "index_bg.wasm"]) {
    const path = join(selected.bundleDirectory, name);
    try {
      await access(path);
    } catch {
      process.stderr.write(`WORKERD_BUNDLE_MISSING ${JSON.stringify({ path })}\n`);
      process.exit(1);
    }
  }
}
const sourceUrl = plan?.textBindings.CENTRAL_SIGN_IN_JWKS_URL;
if (plan !== undefined && sourceUrl === undefined)
  throw new Error("sign-in plan requires CENTRAL_SIGN_IN_JWKS_URL");
const host = await WorkerdStorage.start(
  {
    ...bundle,
    textBindings: { ...bundle.textBindings, ...plan?.textBindings },
    durableObjects: { ...bundle.durableObjects, CENTRAL: "CentralObject" },
  },
  plan === undefined || sourceUrl === undefined || linkBundle === undefined
    ? undefined
    : {
        linkBundle: {
          ...linkBundle,
          textBindings: { ...bundle.textBindings, ...plan.textBindings },
        },
        jwks: { sourceUrl, fixtureUrl: plan.jwksFixtureUrl },
      },
);
process.stdout.write(`${JSON.stringify({ origin: host.origin })}\n`);
try {
  const commands = createInterface({ input: process.stdin, terminal: false });
  for await (const command of commands) {
    if (command !== "close") throw new Error("unknown bridge control");
    commands.close();
    break;
  }
} finally {
  await host.close();
}
