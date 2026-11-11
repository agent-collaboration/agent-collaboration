/** Old125 public CLI receipts on the single commissioned, already-running staging root. */
import { randomUUIDv7 } from "node:crypto";
import { mkdir, writeFile, readdir } from "node:fs/promises";
import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import { artifactSchema } from "../../../declaration.ts";
import { decode, decodeAuthoredJson, encode } from "../../../exact-json.ts";
import { registerCommandRoot } from "../../../machines/command-ledger.ts";
import { command, privateDirectory, qualifyArtifact } from "../../../machines/owned-process.ts";
import { readPrivateText } from "./record.ts";
import { loadStoppedOwnedRoot } from "../../../machines/installation-stopped-root.ts";

export const stagingRoot = "/private/tmp/2026-11-s1-baseline-sunbook-3/router";
export const stagingOrigin = "https://s1-migration.agent-collaboration.com";
export const syntheticReceipts = "/private/tmp/2026-11-s1-staging/s7/synthetic-cases";
const baselineSchema = z.object({
  machine: z.object({ root: z.literal(stagingRoot), cli: artifactSchema, codexHome: z.string() }),
  stagingHome: z.string(),
  sourceHead: z.literal("cbf56bad640ae981900c646ff2ae0b64aaaebd3b"),
  installationId: z.uuid(),
  peerId: z.literal("01a175ad-b72a-7c33-aafb-53b08e82b03f"),
});
const envelope = z.object({ kind: z.literal("result"), result: z.unknown() });
const accepted = z.object({
  record: z.object({
    result: z.object({
      kind: z.literal("decided"),
      decision: z.object({ kind: z.literal("accepted") }),
    }),
  }),
});
const pending = z.object({
  record: z.object({
    result: z.object({ kind: z.enum(["pendingUnreachable", "projectCreationPending"]) }),
  }),
});

export function classifySyntheticWrite(value: unknown): "accepted" | "pending" | "stop" {
  if (accepted.safeParse(value).success) return "accepted";
  if (pending.safeParse(value).success) return "pending";
  return "stop";
}
const forbidden = ["01a14e9e", "01a1635f", "01a193de", "01a1b6f0"];

/** Every scoped identifier supplied to a mutation must belong to this fresh synthetic run. */
export function requireSyntheticScope(
  arguments_: readonly string[],
  owned: ReadonlySet<string>,
): void {
  for (const [index, argument] of arguments_.entries()) {
    if (["--workspace-id", "--board-id", "--project-id"].includes(argument)) {
      const identifier = arguments_[index + 1];
      if (
        identifier === undefined ||
        !owned.has(identifier) ||
        forbidden.some((prefix): boolean => identifier.startsWith(prefix))
      )
        throw new Error("mutation outside newly allocated synthetic scope");
    }
  }
  if (
    ["claim", "enroll", "move", "takeover", "repair", "migrate"].some((entry): boolean =>
      arguments_.includes(entry),
    )
  )
    throw new Error("command exceeds Sunbook synthetic commission");
}

export class SyntheticCli {
  private sequence = 0;
  readonly baseline: z.infer<typeof baselineSchema>;
  readonly actor: {
    readonly kind: "session";
    readonly session: {
      readonly endpoint: { readonly serviceId: string; readonly endpointId: string };
      readonly sessionId: string;
    };
  };
  private readonly release: () => void;
  readonly directory: string;
  private constructor(
    baseline: z.infer<typeof baselineSchema>,
    actor: SyntheticCli["actor"],
    release: () => void,
    directory: string,
  ) {
    this.baseline = baseline;
    this.actor = actor;
    this.release = release;
    this.directory = directory;
  }
  static async open(
    mode:
      | "create"
      | "observeCreated"
      | "independent"
      | "archiveDiagnosis"
      | "retirement"
      | "retirementContinuation" = "create",
    receiptAttempt?: string,
    executionOverlayFile?: string,
  ): Promise<SyntheticCli> {
    const originalBaseline = baselineSchema.parse(
      decodeAuthoredJson(
        await readPrivateText("/private/tmp/2026-11-s1-staging/s7/sunbook-baseline-plan-3.json"),
      ),
    );
    const overlay =
      executionOverlayFile === undefined
        ? undefined
        : await loadStoppedOwnedRoot(
            decodeAuthoredJson(await readPrivateText(executionOverlayFile)),
          );
    if (
      overlay !== undefined &&
      (overlay.declaration.root !== stagingRoot ||
        overlay.baseline.installationId !== originalBaseline.installationId ||
        overlay.baseline.machineId !== originalBaseline.peerId)
    )
      throw new Error("execution overlay belongs to another original baseline");
    const baseline =
      overlay === undefined
        ? originalBaseline
        : {
            ...originalBaseline,
            machine: { ...originalBaseline.machine, cli: overlay.machine.cli },
          };
    const release = await registerCommandRoot(stagingRoot);
    try {
      await mkdir(syntheticReceipts, { mode: 0o700, recursive: true });
      await privateDirectory(syntheticReceipts);
      await qualifyArtifact(baseline.machine.cli, stagingRoot);
      if (
        overlay === undefined &&
        baseline.machine.cli.sha256 !==
          "693485f0c83b7e84bbeb0b1bac5c9f02a3511928da0b251136c6010d782feef7"
      )
        throw new Error("old125 CLI hash differs");
      if (overlay !== undefined) {
        await qualifyOverlayVersion(baseline);
      }
      const actor = {
        kind: "session",
        session: {
          endpoint: { serviceId: baseline.installationId, endpointId: "cursor-local" },
          sessionId: randomUUIDv7(),
        },
      } as const;
      if (receiptAttempt !== undefined && mode !== "retirementContinuation")
        throw new Error("named attempts are limited to the commissioned retirement continuation");
      const suffix = receiptAttempt === undefined ? "" : `-${z.uuidv7().parse(receiptAttempt)}`;
      const directory =
        mode === "create" ? syntheticReceipts : `${syntheticReceipts}/${mode}${suffix}`;
      if (mode !== "create") {
        // No retirement request existed when this continuation was commissioned.
        // A fresh subdirectory preserves the failed read and refuses a second continuation.
        if (
          mode === "observeCreated" &&
          (await readdir(syntheticReceipts)).some((name): boolean => name.includes("retirement"))
        )
          throw new Error("prior retirement requires reconciliation, no replay");
        await mkdir(directory, { mode: 0o700 });
      }
      const cli = new SyntheticCli(baseline, actor, release, directory);
      if (overlay !== undefined)
        await cli.save("execution-overlay", {
          declarationFile: executionOverlayFile,
          originalPlan: overlay.declaration.original.plan,
          originalCli: originalBaseline.machine.cli,
          executionCli: baseline.machine.cli,
          sourceHead: overlay.declaration.execution.sourceHead,
          productVersion: "0.1.125",
          historicalBytesEqual: false,
        });
      return cli;
    } catch (error: unknown) {
      release();
      throw error;
    }
  }
  async save(name: string, value: unknown): Promise<void> {
    if (!/^[a-zA-Z0-9.-]+$/.test(name)) throw new Error("receipt basename required");
    await writeFile(`${this.directory}/${name}.json`, `${encode(value)}\n`, {
      mode: 0o600,
      flag: "wx",
    });
  }
  async input(name: string, value: unknown): Promise<string> {
    await this.save(name, value);
    return `${this.directory}/${name}.json`;
  }
  async execute(label: string, arguments_: readonly string[], admin = false): Promise<unknown> {
    const name = `${String(this.sequence++).padStart(3, "0")}-${label}`;
    const invocation = admin
      ? [...arguments_, "--central", stagingOrigin, "--json"]
      : [...arguments_, "--service-directory", `${stagingRoot}/agent-communication`, "--json"];
    await this.save(`${name}-request`, {
      executable: this.baseline.machine.cli.path,
      arguments: invocation,
      admin,
    });
    const environment = { ...process.env };
    delete environment.CODEX_ROUTER_HOST_INHERITED_LOCK;
    delete environment.CODEX_ROUTER_USE_HOME_DEFAULT;
    // Admin CLI only calls cloudflared + HTTPS; the existing owner's cached sign-in stays in HOME.
    // Board CLI inherits exactly the isolated Host's empty provider HOME and CODEX_HOME.
    if (!admin) {
      environment.HOME = this.baseline.stagingHome;
      environment.CODEX_HOME = this.baseline.machine.codexHome;
    }
    try {
      const result = await command(this.baseline.machine.cli.path, invocation, {
        cwd: stagingRoot,
        environment,
        timeoutSeconds: 60,
      });
      await this.save(`${name}-reply`, result);
      if (result.exit !== 0)
        throw new Error(`old125 CLI refused ${name}; inspect saved reply, no replay`);
      const value = decode(result.stdout);
      return admin ? value : envelope.parse(value).result;
    } catch (error: unknown) {
      await this.save(`${name}-stopped`, {
        message: error instanceof Error ? error.message : String(error),
        replayed: false,
      });
      throw error;
    }
  }
  async write(
    label: string,
    arguments_: readonly string[],
    owned: ReadonlySet<string>,
  ): Promise<void> {
    requireSyntheticScope(arguments_, owned);
    const invocation = [
      ...arguments_,
      "--request-id",
      randomUUIDv7(),
      "--actor",
      encode(this.actor),
    ];
    for (let attempt = 0; attempt < 120; attempt += 1) {
      const reply = await this.execute(`${label}-${attempt}`, invocation);
      if (classifySyntheticWrite(reply) === "accepted") return;
      if (classifySyntheticWrite(reply) !== "pending")
        throw new Error(`write not accepted: ${label}; no replay`);
      // Only the old public API's explicit pending result permits same-request continuation.
      await setTimeout(250);
    }
    throw new Error(`known pending write remains incomplete: ${label}`);
  }
  async writeFile(
    label: string,
    family: "role" | "adr",
    fields: Readonly<Record<string, unknown>>,
    owned: ReadonlySet<string>,
  ): Promise<void> {
    const scope = z
      .object({ projectId: z.string().optional(), workspaceId: z.string().optional() })
      .parse(fields);
    requireSyntheticScope(
      Object.entries(scope).flatMap(([key, value]): string[] =>
        value === undefined ? [] : [key === "projectId" ? "--project-id" : "--workspace-id", value],
      ),
      owned,
    );
    const request = {
      requestId: randomUUIDv7(),
      protocolVersion: 1n,
      actingFor: null,
      actor: this.actor,
      ...fields,
    };
    const path = await this.input(`${label}-payload`, request);
    const reply = await this.execute(label, [
      family,
      family === "role" ? "grant" : "create",
      "--request-file",
      path,
    ]);
    accepted.parse(reply);
  }
  close(): void {
    this.release();
  }
}

async function qualifyOverlayVersion(baseline: z.infer<typeof baselineSchema>): Promise<void> {
  const version = await command(baseline.machine.cli.path, ["--version"], {
    cwd: stagingRoot,
    environment: {
      ...process.env,
      HOME: baseline.stagingHome,
      CODEX_HOME: baseline.machine.codexHome,
    },
    timeoutSeconds: 30,
  });
  if (
    baseline.machine.cli.identifier !== "dev.shravansunder.agent-collaboration.debug" ||
    baseline.machine.cli.team !== "974QD84WVC" ||
    version.exit !== 0 ||
    version.stdout.trim() !== "agent-collaboration 0.1.125"
  )
    throw new Error("qualified old125 execution overlay required");
}
