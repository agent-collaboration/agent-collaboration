/** Controlled synthetic Host probe; never opens or uploads a real project copy. */
import { mkdir, writeFile, readdir } from "node:fs/promises";
import { createInterface } from "node:readline";
import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import { artifactSchema, localMachineSchema, sessionSchema } from "../../../declaration.ts";
import { decode, decodeAuthoredJson, encode } from "../../../exact-json.ts";
import { Installation } from "../../../machines/installation.ts";
import type { CliOutcome } from "../../../clients/router-cli.ts";
import { prepareStagingCodexHome } from "../../../machines/codex-home.ts";
import { prepareStagingUserHome } from "../../../machines/user-home-isolation.ts";
import { OwnedProcess, privateDirectory } from "../../../machines/owned-process.ts";
import { registerCommandRoot } from "../../../machines/command-ledger.ts";
import { Receipt } from "../../../receipt.ts";
import { readPrivateText } from "./record.ts";

const planSchema = z
  .object({
    machine: localMachineSchema,
    stagingHome: z.string().startsWith("/"),
    repository: z.string().startsWith("/"),
    sourceHead: z.string().regex(/^[a-f0-9]{40}$/),
    productVersion: z.literal("0.1.125"),
    managedCodex: artifactSchema,
    installationId: z.uuid(),
    peerId: z.uuidv7(),
    agent: sessionSchema,
    centralPort: z.number().int().min(1024).max(65535),
    evidenceDirectory: z.string().startsWith("/"),
    secretBackend: z.literal("debug-file"),
  })
  .strict();
type ProbePlan = z.infer<typeof planSchema>;
interface ProbeContext {
  plan: ProbePlan;
  installation: Installation;
  central: OwnedProcess;
  receipt: Receipt;
  releaseCentral(): void;
  readSequence: number;
}

async function save(context: ProbeContext, name: string, value: unknown): Promise<void> {
  await context.receipt.save(`${name}-${context.readSequence++}.json`, value);
}
async function prepareInstallation(
  plan: ProbePlan,
): Promise<{ installation: Installation; receipt: Receipt }> {
  if (
    !plan.machine.temporaryParent.startsWith("/private/tmp/2026-11-s1-isolation-") ||
    plan.machine.enrollment.kind !== "unattached" ||
    plan.machine.port === plan.centralPort
  )
    throw new Error("fresh synthetic-only isolation allocation required");
  await mkdir(plan.machine.temporaryParent, { mode: 0o700 });
  await mkdir(plan.machine.nativeCwd, { mode: 0o700 });
  await mkdir(plan.evidenceDirectory, { mode: 0o700 });
  const receipt = new Receipt(plan.evidenceDirectory);
  await receipt.save("probe-plan.json", plan);
  await prepareStagingUserHome({ machine: plan.machine, home: plan.stagingHome });
  await prepareStagingCodexHome({ machine: plan.machine, managedCodex: plan.managedCodex });
  const installation = await Installation.prepare({
    machine: plan.machine,
    repository: plan.repository,
    productVersion: plan.productVersion,
    receipt,
    codexHomePurpose: "staging-isolated",
    stagingHome: plan.stagingHome,
  });
  const collaboration = `${plan.machine.root}/agent-communication`;
  await mkdir(collaboration, { mode: 0o700 });
  await writeFile(
    `${collaboration}/service-identity.json`,
    encode({ version: 1, serviceId: plan.installationId }),
    { mode: 0o600, flag: "wx" },
  );
  await writeFile(
    `${collaboration}/installation-peer.json`,
    encode({ version: 1, peerId: plan.peerId }),
    { mode: 0o600, flag: "wx" },
  );
  return { installation, receipt };
}
async function settleOwned(
  installation: Installation,
  central: OwnedProcess,
  releaseCentral: () => void,
): Promise<void> {
  const results = await Promise.allSettled([installation.close(), central.stop()]);
  releaseCentral();
  const failures = results.flatMap((result): unknown[] =>
    result.status === "rejected" ? [result.reason] : [],
  );
  if (failures.length !== 0)
    throw new AggregateError(failures, "owned isolation-probe cleanup unverified");
}
async function bootstrap(plan: ProbePlan): Promise<ProbeContext> {
  const { installation, receipt } = await prepareInstallation(plan);
  const centralRoot = `${plan.machine.temporaryParent}/central`;
  await mkdir(centralRoot, { mode: 0o700 });
  const releaseCentral = await registerCommandRoot(centralRoot);
  const origin = `http://127.0.0.1:${plan.centralPort}`;
  const environment = {
    ...process.env,
    HOME: plan.stagingHome,
    CODEX_HOME: plan.machine.codexHome,
    RUST_LOG: "off",
  };
  const central = new OwnedProcess({
    executable: plan.machine.router.path,
    arguments: [
      "central",
      "serve",
      "--root",
      centralRoot,
      "--listen",
      `127.0.0.1:${plan.centralPort}`,
      "--public-url",
      origin,
      "--owner-identity",
      plan.agent.sessionId,
      "--allow-plaintext-file-secrets",
    ],
    environment,
    cwd: plan.repository,
    name: "isolation-central",
    receipt,
  });
  try {
    const ready = await central.line(60);
    if (ready !== `central listening: ${origin}`)
      throw new Error("local central readiness differs from declared origin");
    await receipt.save("central-readiness.json", { ready });
    const minted = await installation.runRouterCommand([
      "central",
      "mint",
      "--root",
      centralRoot,
      "--kind",
      "enrollment",
      "--machine-valid-for-days",
      "1",
    ]);
    await receipt.save("central-mint.json", minted);
    if (minted.exit !== 0) throw new Error("local enrollment mint failed; no alternative backend");
    const enrolled = await installation.runRouterCommand([
      "fleet",
      "enroll",
      "--router-root",
      plan.machine.root,
      "--endpoint",
      `${origin}/v1/session`,
      "--label",
      "S1 isolation synthetic probe",
      "--self-hosted-root",
      centralRoot,
      "--allow-plaintext-file-secrets",
    ]);
    await receipt.save("fleet-enroll.json", enrolled);
    if (enrolled.exit !== 0) throw new Error("private-HOME explicit debug-file enrollment failed");
    const status = await installation.runRouterCommand([
      "fleet",
      "status",
      "--router-root",
      plan.machine.root,
    ]);
    await receipt.save("fleet-status.json", status);
    const metadata = z
      .object({
        status: z.object({
          kind: z.literal("valid"),
          enrollment: z.object({ peer: z.literal(plan.peerId) }),
        }),
      })
      .parse(decode(status.stdout));
    if (status.exit !== 0 || metadata.status.kind !== "valid")
      throw new Error("private-HOME enrollment secret reopen failed");
    await installation.start();
    return { plan, installation, central, receipt, releaseCentral, readSequence: 0 };
  } catch (error: unknown) {
    await receipt.save("bootstrap-failure.json", {
      message: error instanceof Error ? error.message : String(error),
    });
    await settleOwned(installation, central, releaseCentral);
    throw error;
  }
}

async function inventories(context: ProbeContext): Promise<void> {
  const endpoints = await context.installation.mcp.call("endpoints_list", {});
  await save(context, "endpoints", endpoints);
  const directory = z
    .object({
      endpoints: z.array(
        z.object({
          endpoint: z.object({ endpointId: z.string() }),
          availability: z.object({ state: z.string(), reason: z.string().optional() }),
          channels: z.array(z.unknown()),
        }),
      ),
    })
    .parse(endpoints);
  for (const provider of ["claude-local", "cursor-local"]) {
    const entry = directory.endpoints.find(
      (entry): boolean => entry.endpoint.endpointId === provider,
    );
    if (
      entry?.availability.state !== "unavailable" ||
      entry.availability.reason !== "disabled in providers.json" ||
      entry.channels.length !== 0
    )
      throw new Error("ACP provider is not explicitly disabled");
  }
  const peerRegistry = `${context.plan.stagingHome}/.claude/sessions`;
  await privateDirectory(peerRegistry);
  if ((await readdir(peerRegistry)).length !== 0)
    throw new Error("private Claude peer registry is nonempty");
  for (const endpointId of ["codex-local", "claude-local", "cursor-local"]) {
    for (const view of ["stored", "loaded"]) {
      const outcome = await context.installation.mcp.outcome(
        endpointId === "codex-local" ? "sessions_list" : "provider_sessions_list",
        {
          endpoint: { serviceId: context.plan.installationId, endpointId },
          view,
          source: "all",
          scope: { kind: "any" },
          pageSize: 100,
        },
      );
      await save(context, `inventory-outcome-${endpointId}-${view}`, outcome);
      if (outcome.kind !== "answered") {
        if (endpointId !== "codex-local" && outcome.kind === "refused") {
          await save(context, `inventory-not-observed-${endpointId}-${view}`, {
            inventoryObserved: false,
            reason:
              "disabled provider inventory refused; adopted isolation proof uses private paths, empty peer registry, repeated lsof and actual delivery outcomes",
          });
          continue;
        }
        throw new Error(
          `provider inventory unconfirmed: ${endpointId}/${view}; typed evidence saved`,
        );
      }
      const reply = outcome.value;
      await save(context, `inventory-${endpointId}-${view}`, reply);
      const parsed = z
        .object({ sessions: z.array(z.unknown()), nextCursor: z.string().nullable().optional() })
        .parse(reply);
      if (parsed.sessions.length !== 0 || parsed.nextCursor != null)
        throw new Error("private provider inventory is nonempty or truncated; no marker submitted");
    }
  }
  await context.installation.check();
}

const control = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("inventories") }).strict(),
  z.object({ kind: z.literal("pauseHost") }).strict(),
  z.object({ kind: z.literal("resumeHost") }).strict(),
  z.object({ kind: z.literal("stop") }).strict(),
  z.object({ kind: z.literal("cli"), arguments: z.array(z.string()).min(1) }).strict(),
  z
    .object({
      kind: z.literal("agentWake"),
      operationId: z.uuidv7(),
      target: sessionSchema,
      text: z.string().startsWith("S1-ISOLATION-PROBE "),
    })
    .strict(),
  z
    .object({
      kind: z.literal("read"),
      tool: z.enum(["router_show", "feed_read", "project_show", "wake_show", "wake_list"]),
      parameters: z.record(z.string(), z.unknown()),
    })
    .strict(),
]);
export async function retainProbeCliOutcome(input: {
  arguments: readonly string[];
  outcome: CliOutcome;
  retain(name: string, value: unknown): Promise<void>;
}): Promise<void> {
  await input.retain("cli-outcome", { arguments: input.arguments, outcome: input.outcome });
  if (input.outcome.kind !== "answered")
    throw new Error(
      "Router CLI refused or unconfirmed; typed outcome and exact arguments retained in cli-outcome evidence",
    );
  await input.retain("cli-result", input.outcome.value);
}
export function probeCliArguments(arguments_: readonly string[]): readonly string[] {
  if (
    arguments_.some(
      (argument): boolean =>
        argument === "--json" ||
        argument.startsWith("--json=") ||
        argument === "--service-directory" ||
        argument.startsWith("--service-directory="),
    )
  )
    throw new Error(
      "the probe wrapper supplies --json and --service-directory; omit both from control arguments",
    );
  return arguments_;
}
async function run(plan: ProbePlan): Promise<void> {
  process.umask(0o077);
  const context = await bootstrap(plan);
  const controls = createInterface({ input: process.stdin, crlfDelay: Infinity });
  const terminate = (): void => {
    controls.close();
  };
  process.once("SIGTERM", terminate);
  process.once("SIGINT", terminate);
  let active = true,
    paused = false,
    sampleFailure: unknown;
  let pendingSample: Promise<void> = Promise.resolve();
  const monitor = (async (): Promise<void> => {
    while (active) {
      if (!paused) {
        pendingSample = context.installation.check();
        try {
          await pendingSample;
        } catch (error: unknown) {
          sampleFailure = error;
          controls.close();
          return;
        }
      }
      await setTimeout(500);
    }
  })();
  try {
    await inventories(context);
    process.stdout.write(
      `${encode({ kind: "syntheticHostReady", root: plan.machine.root, credentialBackendObserved: "debug-file", privateRoutePreflight: true, deliveryProbeRun: false })}\n`,
    );
    for await (const line of controls) {
      const operation = control.parse(decode(line));
      if (operation.kind === "stop") break;
      if (operation.kind === "inventories") await inventories(context);
      else if (operation.kind === "pauseHost") {
        paused = true;
        await pendingSample;
        await context.installation.stopHost();
      } else if (operation.kind === "resumeHost") {
        await context.installation.startHost();
        paused = false;
      } else if (operation.kind === "cli") {
        await save(context, "cli-control", {
          arguments: operation.arguments,
          wrapperOwns: ["--json", "--service-directory"],
        });
        probeCliArguments(operation.arguments);
        const outcome = await context.installation.cli.outcome(operation.arguments);
        await retainProbeCliOutcome({
          arguments: operation.arguments,
          outcome,
          retain: (name, value) => save(context, name, value),
        });
      } else if (operation.kind === "agentWake")
        await save(
          context,
          "agent-wake-created",
          await context.installation.mcp.call("wake_send", {
            operationId: operation.operationId,
            requestedBy: { kind: "session", ...plan.agent },
            replace: false,
            message: {
              target: operation.target,
              content: { kind: "agent", sender: plan.agent, text: operation.text },
              delivery: "auto",
            },
            timing: { kind: "after", seconds: 1 },
            expiry: { kind: "none" },
          }),
        );
      else
        await save(
          context,
          `read-${operation.tool}`,
          await context.installation.mcp.call(operation.tool, operation.parameters),
        );
      process.stdout.write(`${encode({ kind: "probeStepRecorded", step: operation.kind })}\n`);
    }
  } finally {
    process.removeListener("SIGTERM", terminate);
    process.removeListener("SIGINT", terminate);
    active = false;
    controls.close();
    await monitor;
    await settleOwned(context.installation, context.central, context.releaseCentral);
  }
  if (sampleFailure !== undefined) throw sampleFailure;
}
if (import.meta.main) {
  const path = process.argv[2];
  if (path === undefined || process.argv.length !== 3) process.exitCode = 3;
  else
    void readPrivateText(path)
      .then((text): ProbePlan => planSchema.parse(decodeAuthoredJson(text)))
      .then(run)
      .catch((error: unknown): void => {
        process.stderr.write(
          `${encode({ kind: "isolationProbeIncomplete", message: error instanceof Error ? error.message : String(error) })}\n`,
        );
        process.exitCode = 1;
      });
}
