/** Actual v0.1.134-source Host, isolated native home and plaintext debug enrollment. */
import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import type { CompatibilityPlan } from "./compatibility-plan.ts";
import { CompatibilityWireObservation } from "./wire-observation.ts";
import { FixtureEdge } from "../../../access/fixture-edge.ts";
import { fixtureMachineAssertion } from "../../../access/fixture-assertion.ts";
import { SessionGate } from "../../../access/session-gate.ts";
import { RouterMcp } from "../../../clients/router-mcp.ts";
import { RouterNative } from "../../../clients/router-native.ts";
import { boardIdentity } from "../../../clients/board-requests.ts";
import { decode, encode } from "../../../exact-json.ts";
import {
  command,
  fingerprint,
  OwnedProcess,
  qualifyArtifact,
} from "../../../machines/owned-process.ts";
import type { Receipt } from "../../../receipt.ts";
import { persistEmptyRecipient, readUnloadedPersistedRecipient } from "../passive-recipient.ts";

export async function freePort(): Promise<number> {
  const server = createServer();
  await new Promise<void>((resolve, reject): void => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  await new Promise<void>((resolve, reject): void => {
    server.close((error): void => (error ? reject(error) : resolve()));
  });
  if (
    address === null ||
    typeof address === "string" ||
    [8787, 8788, 19741, 19742, 19743, 43127].includes(address.port)
  )
    throw new Error("nonreserved free loopback port required");
  return address.port;
}

export class FixtureInstallation {
  readonly input: {
    name: string;
    runtime: string;
    port: number;
    plan: CompatibilityPlan;
    receipt: Receipt;
  };
  readonly wire = new CompatibilityWireObservation();
  readonly root: string;
  readonly home: string;
  readonly serviceDirectory: string;
  readonly environment: NodeJS.ProcessEnv;
  private edge: FixtureEdge | undefined;
  gate: SessionGate | undefined;
  private host: OwnedProcess | undefined;
  private client: RouterMcp | undefined;
  private native: RouterNative | undefined;
  private readonly nativeConnections: RouterNative[] = [];
  private generation = 0;
  recipient: { endpoint: { serviceId: string; endpointId: string }; sessionId: string } | undefined;
  private constructor(input: {
    name: string;
    runtime: string;
    port: number;
    plan: CompatibilityPlan;
    receipt: Receipt;
  }) {
    this.input = input;
    this.root = `${input.runtime}/${input.name}`;
    this.home = `${this.root}/codex`;
    this.serviceDirectory = `${this.root}/agent-communication`;
    this.environment = {
      HOME: `${input.runtime}/home-${input.name}`,
      PATH: "/usr/bin:/bin:/usr/sbin:/sbin",
      CODEX_HOME: this.home,
      TMPDIR: input.runtime,
      CODEX_ROUTER_DEBUG_APP_SERVER_SOCKET: `${this.root}/n/s`,
      CODEX_INTERNAL_APP_SERVER_REMOTE_CONTROL_DISABLED: "1",
      DIRENV_DISABLE: "1",
      RUST_LOG: "off",
    };
  }
  static async prepare(input: {
    name: string;
    runtime: string;
    port: number;
    plan: CompatibilityPlan;
    receipt: Receipt;
    central: string;
    ownerAssertion: string;
  }): Promise<FixtureInstallation> {
    const machine = new FixtureInstallation(input);
    try {
      await machine.prepareState(input);
      return machine;
    } catch (error: unknown) {
      await machine.close();
      throw error;
    }
  }
  private async prepareState(input: { central: string; ownerAssertion: string }): Promise<void> {
    const { plan, port } = this.input;
    await mkdir(this.root, { mode: 0o700 });
    await mkdir(`${this.input.runtime}/home-${this.input.name}`, { mode: 0o700 });
    await mkdir(`${this.root}/n`, { mode: 0o700 });
    await mkdir(`${this.home}/packages/standalone/current`, { recursive: true, mode: 0o700 });
    const stagedNative = `${this.home}/packages/standalone/current/codex`;
    await copyFile(plan.native.path, stagedNative);
    await qualifyArtifact({ ...plan.native, path: stagedNative });
    const config = `model = "gpt-6-luna"\nmodel_provider = "codex-router-debug"\ncli_auth_credentials_store = "ephemeral"\n[features]\nenable_request_compression = false\n[model_providers.codex-router-debug]\nname = "OpenAI"\nbase_url = "http://127.0.0.1:${port}/v1"\nwire_api = "responses"\nrequires_openai_auth = false\nsupports_websockets = true\n`;
    await writeFile(`${this.home}/config.toml`, config, { mode: 0o600, flag: "wx" });
    // v0.1.134's Host projection accepts provider/model settings only. The
    // native home's ephemeral credential policy stays in its own config.
    await writeFile(
      `${this.home}/codex-router-debug.config.toml`,
      config.replace('cli_auth_credentials_store = "ephemeral"\n', ""),
      { mode: 0o600, flag: "wx" },
    );
    await writeFile(
      `${this.root}/providers.json`,
      encode({
        version: 1,
        providers: {
          claude: { enabled: false, executable: null, arguments: [] },
          cursor: { enabled: false, executable: null, arguments: [] },
        },
      }),
      { mode: 0o600, flag: "wx" },
    );
    const keyPath = `${plan.repository}/crates/collaboration-central-conformance/fixtures/access-test-only-rsa.pk8`;
    const accessId = `verification-${boardIdentity()}.access`;
    const accessSecret = boardIdentity();
    const ownerToken = boardIdentity();
    const expiresAt = new Date(Math.floor(Date.parse(plan.deadline) / 1000) * 1000).toISOString();
    this.edge = await FixtureEdge.start({
      upstream: input.central,
      assertion: await fixtureMachineAssertion({
        key: { path: keyPath, sha256: await fingerprint(keyPath) },
        accessClientId: accessId,
        expiresAt,
      }),
      owner: { token: ownerToken, assertion: input.ownerAssertion },
      accessId,
      accessSecret,
    });
    this.gate = await SessionGate.start(
      `${this.edge.origin}/v1/session`,
      (frame): void => this.wire.observe(frame),
      (frame): void => this.wire.observeClient(frame),
    );
    const secretFile = `${this.root}/fixture-access-secret`;
    await writeFile(secretFile, accessSecret, { mode: 0o600, flag: "wx" });
    await qualifyArtifact(plan.oldRouter);
    const enrolled = await command(
      plan.oldRouter.path,
      [
        "fleet",
        "enroll",
        "--router-root",
        this.root,
        "--endpoint",
        `${this.gate.origin}/v1/session`,
        "--label",
        `D3 ${this.input.name}`,
        "--access-client-id",
        accessId,
        "--access-client-secret-file",
        secretFile,
        "--expires-at",
        expiresAt,
        "--owner-token-env",
        "D3_FIXTURE_OWNER_TOKEN",
        "--allow-plaintext-file-secrets",
      ],
      {
        environment: { ...this.environment, D3_FIXTURE_OWNER_TOKEN: ownerToken },
        timeoutSeconds: 60,
      },
    );
    // Do not retain credential-bearing stdout or inherited environment.
    await this.input.receipt.record("d3EnrollmentExit", {
      machine: this.input.name,
      exit: enrolled.exit,
    });
    if (enrolled.exit !== 0) throw new Error(`D3 isolated enrollment failed: ${enrolled.stderr}`);
    z.object({
      decision: z.object({ kind: z.literal("machineEnrolled") }),
      activation: z.literal("nextRouterStart"),
    }).parse(decode(enrolled.stdout));
  }
  get mcp(): RouterMcp {
    if (this.client === undefined) throw new Error("fixture Host not ready");
    return this.client;
  }
  async start(): Promise<void> {
    if (this.host !== undefined) throw new Error("fixture Host already active");
    const { plan, receipt, port, name } = this.input;
    await qualifyArtifact(plan.oldRouter);
    const previous = this.gate?.audit.snapshot().welcomes ?? 0;
    this.host = new OwnedProcess({
      executable: plan.oldRouter.path,
      arguments: [
        "host",
        "--router-root",
        this.root,
        "--port",
        String(port),
        "--mcp-bind",
        "127.0.0.1:0",
        "--require-debug-isolation",
      ],
      environment: this.environment,
      cwd: this.root,
      name: `d3-host-${name}-${this.generation++}`,
      receipt,
    });
    const deadline = Math.min(Date.now() + 60_000, Date.parse(plan.deadline));
    let lastError: unknown;
    while (Date.now() < deadline) {
      try {
        const response = await fetch(`http://127.0.0.1:${port}/healthz`, {
          signal: AbortSignal.timeout(2000),
        });
        z.object({ binary_version: z.literal("0.1.134") }).parse(await response.json());
        const manifest = z
          .object({
            serviceVersion: z.literal("0.1.134"),
            serviceId: z.string(),
            mcp: z.object({ url: z.url() }),
          })
          .parse(decode(await readFile(`${this.serviceDirectory}/service.json`, "utf8")));
        const client = new RouterMcp(manifest.mcp.url);
        const endpoints = z
          .object({
            endpoints: z.array(
              z.object({
                endpoint: z.object({ endpointId: z.string() }),
                availability: z.object({ state: z.string() }),
              }),
            ),
          })
          .parse(await client.call("endpoints_list", {}, 3));
        if (
          !endpoints.endpoints.some(
            (endpoint): boolean =>
              endpoint.endpoint.endpointId === "codex-local" &&
              endpoint.availability.state === "available",
          )
        )
          throw new Error("native endpoint unavailable");
        await this.gate?.ready(previous);
        this.client = client;
        await qualifyArtifact(plan.oldCli);
        this.native = await RouterNative.connectAtService({
          cli: plan.oldCli,
          serviceDirectory: this.serviceDirectory,
          repository: plan.repository,
          environment: this.environment,
          receipt,
        });
        this.nativeConnections.push(this.native);
        if (this.recipient === undefined) await this.createRecipient(manifest.serviceId);
        await this.unload();
        this.host.capture();
        await receipt.record("d3HostReady", {
          machine: name,
          pid: this.host.pid,
          recipient: this.recipient,
          source: "released 0.1.134 source, debug build",
        });
        return;
      } catch (error: unknown) {
        lastError = error;
        // Once connected to native, a failed mutation is not replayable.
        if (this.native !== undefined) throw error;
        if (this.host.child.exitCode !== null) break;
        await setTimeout(100);
      }
    }
    throw new Error(`D3 Host readiness failed: ${String(lastError)}`);
  }
  private async createRecipient(serviceId: string): Promise<void> {
    if (this.native === undefined) throw new Error("native connection absent");
    const started = z
      .object({
        model: z.literal("gpt-6-luna"),
        modelProvider: z.literal("codex-router-debug"),
        reasoningEffort: z.literal("low"),
        thread: z.object({ id: z.string(), ephemeral: z.literal(false) }),
      })
      .parse(
        await this.native.rpc("thread/start", {
          cwd: this.root,
          model: "gpt-6-luna",
          modelProvider: "codex-router-debug",
          approvalPolicy: "never",
          sandbox: "read-only",
          config: { model_reasoning_effort: "low", thread_unload_delay_secs: 0 },
          developerInstructions: "Passive compatibility recipient. No model input is authorized.",
        }),
      );
    this.recipient = {
      endpoint: { serviceId, endpointId: "codex-local" },
      sessionId: started.thread.id,
    };
    await this.persistRecipientWithoutInput(started.thread.id);
  }
  private async persistRecipientWithoutInput(threadId: string): Promise<void> {
    if (this.native === undefined) throw new Error("native connection absent");
    // Actual Codex0.166 archive persists a loaded empty thread; unarchive
    // restores it without inference. No rollout contents are synthesized.
    const { archived, restored } = await persistEmptyRecipient({ native: this.native, threadId });
    await this.input.receipt.record("d3RecipientPersistedWithoutInput", {
      recipient: this.recipient,
      route: "thread/archive then thread/unarchive",
      archived,
      restored,
    });
  }
  async unload(): Promise<void> {
    if (this.native === undefined || this.recipient === undefined)
      throw new Error("native recipient absent");
    const stored = await readUnloadedPersistedRecipient({
      native: this.native,
      threadId: this.recipient.sessionId,
      home: this.home,
    });
    await this.input.receipt.record("d3UnloadedRecipientReadback", {
      recipient: this.recipient,
      stored,
    });
    this.requireNoTurns();
  }
  async stop(): Promise<void> {
    const failures: unknown[] = [];
    if (this.native !== undefined) {
      try {
        await this.native.close();
        this.native = undefined;
      } catch (error: unknown) {
        failures.push(error);
      }
    }
    if (this.host !== undefined) {
      try {
        await this.host.stop();
        this.host = undefined;
      } catch (error: unknown) {
        failures.push(error);
      }
    }
    this.client = undefined;
    if (failures.length > 0) throw new AggregateError(failures, "native/Host cleanup incomplete");
  }
  requireNoTurns(): void {
    if (
      this.nativeConnections.some((native): boolean =>
        native.events.some((event): boolean => event.method === "turn/started"),
      )
    )
      throw new Error("unexpected model turn in no-input compatibility run");
  }
  async close(): Promise<void> {
    const failures: unknown[] = [];
    for (const stop of [
      (): Promise<void> => this.stop(),
      (): Promise<void> => this.gate?.close() ?? Promise.resolve(),
      (): Promise<void> => this.edge?.close() ?? Promise.resolve(),
    ])
      try {
        await stop();
      } catch (error: unknown) {
        failures.push(error);
      }
    if (failures.length > 0) throw new AggregateError(failures, "fixture cleanup incomplete");
  }
}
