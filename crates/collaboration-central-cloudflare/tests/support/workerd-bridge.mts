// Starts the established loader. Requests and business assertions belong to Rust.
import { createInterface } from "node:readline";
import { access, readFile } from "node:fs/promises";
import { join } from "node:path";
import {
  WorkerdStorage,
  centralTestBundle,
  centralLinkTestBundle,
  workerdSignInPlanSchema,
  workerdBridgeCommandSchema,
  workerdBridgeObservationSchema,
  workerdClockAcknowledgementSchema,
  workerdLauncherTimeSchema,
} from "../../../../verification/targets/workerd-storage.ts";
const arguments_ = process.argv.slice(2);
let milliseconds: number | undefined;
const launcherIndex = arguments_.indexOf("--launcher-time-ms");
if (launcherIndex !== -1) {
  const initialTime = arguments_[launcherIndex + 1];
  const parsed = workerdLauncherTimeSchema.safeParse(Number(initialTime));
  if (initialTime === undefined || !/^(0|[1-9]\d*)$/.test(initialTime) || !parsed.success)
    throw new Error("launcher time must be a supported integer millisecond");
  milliseconds = parsed.data;
  arguments_.splice(launcherIndex, 2);
}
const [option, planPath] = arguments_;
if (
  arguments_.length !== 0 &&
  (arguments_.length !== 2 || option !== "--signin-plan" || planPath === undefined)
)
  throw new Error(
    "usage: workerd-bridge.mts [--signin-plan <fixture-plan.json>] [--launcher-time-ms <milliseconds>]",
  );
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
const organizations = new Set<string>();
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
  milliseconds === undefined
    ? undefined
    : (organization: string): number | null => {
        organizations.add(organization);
        return milliseconds ?? null;
      },
);
process.stdout.write(`${JSON.stringify({ origin: host.origin })}\n`);

async function control(
  organization: string,
  operation: "clock" | "raw-signin-observe",
  body: unknown,
): Promise<unknown> {
  const prefix = plan === undefined ? "" : "/public";
  return host.dispatchJson(`${prefix}/test-support/workspace/${organization}/${operation}`, body);
}

async function answer(line: string): Promise<unknown> {
  let input: unknown;
  try {
    input = JSON.parse(line);
  } catch {
    return { answer: "refused", error: "invalid bridge command" };
  }
  const parsed = workerdBridgeCommandSchema.safeParse(input);
  if (!parsed.success || milliseconds === undefined)
    return { answer: "refused", error: "invalid bridge command" };
  try {
    if ("setTimeMs" in parsed.data) {
      milliseconds = parsed.data.setTimeMs;
      const live = [...organizations];
      try {
        for (const organization of live) {
          workerdClockAcknowledgementSchema.parse(
            await control(organization, "clock", milliseconds),
          );
        }
      } catch {
        // An uncertain push invalidates this fixture run; never retry or pretend rollback.
        return { answer: "failed", error: "bridge time may be partly applied" };
      }
      return { answer: "timeSet", setTimeMs: milliseconds, liveFleets: live.length };
    }
    const organization = parsed.data.observe;
    const observation = workerdBridgeObservationSchema.parse(
      organizations.has(organization)
        ? await control(organization, "raw-signin-observe", {})
        : JSON.parse(
            await readFile(
              new URL(
                "../../../collaboration-central-core/src/signin_observation_empty.json",
                import.meta.url,
              ),
              "utf8",
            ),
          ),
    );
    return { answer: "observed", organization, observation };
  } catch {
    // Never serialize a failed request, compact statement or raw response here.
    return { answer: "failed", error: "bridge control failed" };
  }
}

try {
  const commands = createInterface({ input: process.stdin, terminal: false });
  for await (const command of commands) {
    if (command === "close") {
      commands.close();
      break;
    }
    if (command.trim() !== "") process.stderr.write(`${JSON.stringify(await answer(command))}\n`);
  }
} finally {
  await host.close();
}
