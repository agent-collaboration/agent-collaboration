/** Read the product's Host status text on a declared owned staging root. */
import { createHash } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { z } from "zod";
import { artifactSchema } from "../../../declaration.ts";
import { encode } from "../../../exact-json.ts";
import { command, privateDirectory, qualifyArtifact } from "../../../machines/owned-process.ts";
import { registerCommandRoot } from "../../../machines/command-ledger.ts";
import { identifier, routerTarget } from "./contracts.ts";
import { readPrivateJson } from "./record.ts";

const expectation = z.object({ peer: identifier, origin: z.url().startsWith("https://") }).strict();
export function qualifyConnectedHost(
  text: string,
  rawExpected: unknown,
): {
  peer: string;
  origin: string;
  instance: bigint;
  connection: bigint;
  revision: bigint;
} {
  const expected = expectation.parse(rawExpected);
  const lines = text.split(/\r?\n/),
    central = lines.filter((line): boolean => line.startsWith("central: "));
  const peers = lines.filter((line): boolean => line.startsWith("central peer: "));
  const [state] = central,
    [peerLine] = peers;
  if (central.length !== 1 || peers.length !== 1 || state === undefined || peerLine === undefined)
    throw new Error("one actual central status and peer line required");
  const connected = state.match(
    /^central: connected since .+; last message from central .+; instance (\d+); connection (\d+); revision (\d+)$/,
  );
  const identity = peerLine.match(/^central peer: ([a-f0-9-]+); endpoint: (\S+)$/);
  if (
    connected?.[1] === undefined ||
    connected[2] === undefined ||
    connected[3] === undefined ||
    identity?.[1] === undefined ||
    identity[2] === undefined
  )
    throw new Error("Host does not report a current connected Session");
  const endpoint = new URL(identity[2]),
    origin = new URL(expected.origin);
  if (
    identity[1] !== expected.peer ||
    endpoint.origin !== origin.origin ||
    endpoint.pathname !== "/" ||
    endpoint.search !== "" ||
    endpoint.hash !== "" ||
    endpoint.username !== "" ||
    endpoint.password !== ""
  )
    throw new Error("connected Host peer or staging origin mismatch");
  return {
    peer: expected.peer,
    origin: endpoint.origin,
    instance: BigInt(connected[1]),
    connection: BigInt(connected[2]),
    revision: BigInt(connected[3]),
  };
}

const planSchema = z
  .object({
    router: artifactSchema,
    target: routerTarget,
    routerRoot: z.string().startsWith("/"),
    evidenceDirectory: z.string().startsWith("/"),
    expected: expectation,
  })
  .strict();

export async function recordHostStatus(rawPlan: unknown): Promise<string> {
  const plan = planSchema.parse(rawPlan);
  if (/(^|\/)\.(codex|codex-router|claude)(\/|$)/.test(plan.routerRoot))
    throw new Error("owned staging debug root required");
  await privateDirectory(plan.routerRoot);
  await qualifyArtifact(plan.router);
  const version = await command(plan.router.path, ["--version"]);
  if (version.exit !== 0 || version.stdout.trim() !== `codex-router ${plan.target.version}`)
    throw new Error("declared Router version differs from signed artifact");
  await mkdir(plan.evidenceDirectory, { mode: 0o700 });
  await privateDirectory(plan.evidenceDirectory);
  const arguments_ = ["host", "status", "--router-root", plan.routerRoot];
  const release = await registerCommandRoot(plan.routerRoot);
  const outcome = await command(plan.router.path, arguments_, { timeoutSeconds: 45 }).finally(
    release,
  );
  const raw = encode({
    executable: plan.router.path,
    arguments: arguments_,
    target: plan.target,
    ...outcome,
  });
  const sourcePath = `${plan.evidenceDirectory}/host-status-command.json`;
  await writeFile(sourcePath, raw, { mode: 0o600, flag: "wx" });
  if (outcome.exit !== 0) throw new Error("Host status failed; raw outcome retained");
  const connected = qualifyConnectedHost(outcome.stdout, plan.expected);
  await qualifyArtifact(plan.router);
  const output = `${plan.evidenceDirectory}/host-connected.json`;
  await writeFile(
    output,
    encode({
      kind: "hostConnectedObserved",
      target: plan.target,
      router: plan.router,
      connected,
      source: {
        artifact: sourcePath,
        sha256: createHash("sha256").update(raw).digest("hex"),
        method: "machineAuthentication",
      },
      renewalObserved: false,
      enrollmentContinuityObserved: false,
    }),
    { mode: 0o600, flag: "wx" },
  );
  return output;
}

if (import.meta.main) {
  const path = process.argv[2];
  if (path === "--help" && process.argv.length === 3)
    process.stdout.write(
      "host-status.ts PRIVATE_PLAN.json\nRead signed declared Router host status at an owned staging debug root. Captures real connected peer/origin/counters; does not claim renewal or enrollment continuity.\n",
    );
  else if (path === undefined || process.argv.length !== 3) process.exitCode = 3;
  else
    void readPrivateJson(path)
      .then(recordHostStatus)
      .then((output): void => {
        process.stdout.write(`${encode({ kind: "hostStatusRecorded", output })}\n`);
      })
      .catch((): void => {
        process.stderr.write("Host status observation failed; private inputs retained\n");
        process.exitCode = 3;
      });
}
