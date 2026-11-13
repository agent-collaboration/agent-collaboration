import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import { createServer } from "node:net";
import type { Machine } from "../declaration.ts";
import type { Receipt } from "../receipt.ts";
import { decode, encode } from "../exact-json.ts";
import { RouterMcp } from "../clients/router-mcp.ts";
import { RouterCli } from "../clients/router-cli.ts";
import {
  OwnedProcess,
  privateDirectory,
  qualifyArtifact,
  shellQuote,
  command,
  fingerprint,
} from "./owned-process.ts";
import { Remote, type RemoteProcess } from "./remote.ts";
import {
  CodexHomeGuard,
  type ConfigurationEligibility,
  type CodexHomePurpose,
} from "./codex-home.ts";
import {
  registerCommandRoot,
  markCommandBaseline,
  enrollInvocationsSince,
} from "./command-ledger.ts";
import { validateStagingUserHome, observeRuntimeHomeIsolation } from "./user-home-isolation.ts";
import {
  identitySchema,
  upgradeBaselineSchema,
  enrollmentStatus,
  validateUpgradeEnrollment,
  type InstallationIdentity,
  type UpgradeBaseline,
} from "./installation-baseline.ts";
import {
  sanitizedInstallationFailure,
  type InstallationStage,
  type InstallationFailure,
} from "./installation-failure.ts";
import {
  loadStoppedOwnedRoot,
  inspectStoppedOwnedRoot,
  assertStoppedFileIdentities,
  type StoppedOwnedRootDeclaration,
} from "./installation-stopped-root.ts";

type Child =
  | { readonly kind: "local"; readonly process: OwnedProcess }
  | { readonly kind: "ssh"; readonly process: RemoteProcess };
type State =
  | { readonly kind: "prepared" }
  | { readonly kind: "closed" }
  | { readonly kind: "routerRunning"; readonly router: Child }
  | { readonly kind: "hostStarting"; readonly router: Child; readonly host: Child }
  | {
      readonly kind: "ready";
      readonly router: Child;
      readonly host: Child;
      readonly mcp: RouterMcp;
    };
/** Releases before 0.1.142 read provider settings from providers.json, not settings.jsonc. */
export function readsLegacyProvidersFile(productVersion: string): boolean {
  const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(productVersion);
  if (match === null) throw new Error(`semantic product version required: ${productVersion}`);
  const [major, minor, patch] = [Number(match[1]), Number(match[2]), Number(match[3])];
  return major === 0 && (minor < 1 || (minor === 1 && patch < 142));
}
/** Both root-file renames shipped in the same 0.1.142 storage cutover. */
export function installationProxyDatabase(root: string, productVersion: string): string {
  return `${root}/${readsLegacyProvidersFile(productVersion) ? "state.sqlite" : "provider-proxy.sqlite"}`;
}
export class Installation {
  readonly machine: Machine;
  readonly remote: Remote | undefined;
  readonly cli: RouterCli;
  private readonly input: {
    repository: string;
    productVersion: string;
    receipt: Receipt;
    stagingHome?: string;
  };
  private readonly guard: CodexHomeGuard;
  private readonly children = new Set<Child>();
  private state: State = { kind: "prepared" };
  private generation = 0;
  private readonly releaseCommandRoot: () => void;
  private continuation:
    | {
        kind: "stopped" | "owned";
        declaration: StoppedOwnedRootDeclaration;
        baseline: UpgradeBaseline;
      }
    | undefined;
  private constructor(input: {
    machine: Machine;
    remote: Remote | undefined;
    guard: CodexHomeGuard;
    repository: string;
    productVersion: string;
    receipt: Receipt;
    releaseCommandRoot: () => void;
    stagingHome?: string;
    continuation?: {
      kind: "stopped" | "owned";
      declaration: StoppedOwnedRootDeclaration;
      baseline: UpgradeBaseline;
    };
  }) {
    this.machine = input.machine;
    this.remote = input.remote;
    this.guard = input.guard;
    this.input = input;
    this.releaseCommandRoot = input.releaseCommandRoot;
    this.continuation = input.continuation;
    this.cli = new RouterCli(
      input.machine.cli,
      `${input.machine.root}/agent-communication`,
      input.remote,
    );
  }
  static async prepare(input: {
    machine: Machine;
    repository: string;
    productVersion: string;
    receipt: Receipt;
    codexHomePurpose?: CodexHomePurpose;
    stagingHome?: string;
  }): Promise<Installation> {
    const { machine } = input;
    if (
      resolve(machine.root) !== machine.root ||
      !machine.root.startsWith(`${machine.temporaryParent}/`) ||
      /(^|\/)\.(codex|codex-router|claude)(\/|$)/.test(machine.root) ||
      Buffer.byteLength(`${machine.root}/n/app-server.sock`) > 100
    )
      throw new Error("short, isolated canonical Router root required");
    await qualifyArtifact(machine.router);
    await qualifyArtifact(machine.cli);
    const remote = machine.kind === "ssh" ? new Remote(machine) : undefined;
    if (input.codexHomePurpose === "staging-isolated") {
      if (input.stagingHome === undefined)
        throw new Error("staging isolation requires a private child HOME");
      await validateStagingUserHome({ machine, home: input.stagingHome });
    } else if (input.stagingHome !== undefined)
      throw new Error("private child HOME requires explicit staging-isolated purpose");
    const guard = await CodexHomeGuard.capture(machine, remote, input.codexHomePurpose);
    if (remote === undefined) {
      await privateDirectory(machine.temporaryParent);
      await mkdir(machine.root, { mode: 0o700 });
      await privateDirectory(machine.root);
      await mkdir(`${machine.root}/n`, { mode: 0o700 });
    } else {
      await remote.prepare();
      await remote.transfer(machine.router, remote.pathFor(machine.router));
      await remote.transfer(machine.cli, remote.pathFor(machine.cli));
      z.object({ kind: z.literal("nativeDirectoryPrepared") }).parse(
        await remote.json(
          `umask 077; mkdir ${shellQuote(`${machine.root}/n`)}; printf '%s' ${shellQuote(encode({ kind: "nativeDirectoryPrepared" }))}`,
        ),
      );
    }
    // Provider isolation must reach the file the declared product reads: releases before
    // 0.1.142 read providers.json; later ones read settings.jsonc.
    const disabledProviders = {
      claude: { enabled: false, executable: null, arguments: [] },
      cursor: { enabled: false, executable: null, arguments: [] },
    };
    const [settingsFile, settingsContent] = readsLegacyProvidersFile(input.productVersion)
      ? ["providers.json", encode({ version: 1, providers: disabledProviders })]
      : ["settings.jsonc", encode({ providers: { version: 1, providers: disabledProviders } })];
    if (remote === undefined)
      await writeFile(`${machine.root}/${settingsFile}`, settingsContent, {
        mode: 0o600,
        flag: "wx",
      });
    else await remote.writePrivate(settingsFile, settingsContent);
    const releaseCommandRoot =
      remote === undefined ? await registerCommandRoot(machine.root) : (): void => {};
    return new Installation({ ...input, remote, guard, releaseCommandRoot });
  }
  get mcp(): RouterMcp {
    if (this.state.kind !== "ready") throw new Error("installation Host is not ready");
    return this.state.mcp;
  }
  /** Continue only a declared historical root whose original owner has already settled. */
  static async continueStoppedOwned(input: {
    declaration: unknown;
    receipt: Receipt;
  }): Promise<Installation> {
    const initial = await loadStoppedOwnedRoot(input.declaration);
    const release = await registerCommandRoot(initial.declaration.root);
    try {
      const context = await inspectStoppedOwnedRoot(initial.declaration);
      const guard = await CodexHomeGuard.captureExistingStaging({
        machine: context.machine,
        home: context.original.stagingHome,
        expected: context.declaration.homes,
      });
      await input.receipt.record("stoppedOwnedContinuationPrepared", {
        declaration: context.declaration,
        originalBaseline: context.baseline,
        originalHostWindow: context.declaration.original.hostWindow,
        historicalInodeProofClaimed: false,
        ownedChildren: [],
      });
      return new Installation({
        machine: context.machine,
        remote: undefined,
        guard,
        repository: context.original.repository,
        productVersion: context.declaration.execution.productVersion,
        receipt: input.receipt,
        releaseCommandRoot: release,
        stagingHome: context.original.stagingHome,
        continuation: {
          kind: "stopped",
          declaration: context.declaration,
          baseline: context.baseline,
        },
      });
    } catch (error: unknown) {
      release();
      throw error;
    }
  }

  /** Refuse the retired in-place upgrade before any inspection or child shutdown. */
  static async resumeStoppedUpgrade(_input: unknown): Promise<never> {
    throw new Error(
      "retired: the updater was replaced by scripts/one-off/2026-11-convert-router-root.ts",
    );
  }
  configurationEligibility(): ConfigurationEligibility {
    return this.guard.eligibility;
  }
  get enrollmentLabel(): string {
    switch (this.machine.enrollment.kind) {
      case "fixture":
        return this.machine.name;
      case "access":
        return this.machine.enrollment.label;
      case "unattached":
        throw new Error("unattached installation has no enrolled label");
    }
  }
  private async read(path: string): Promise<string> {
    if (!path.startsWith(`${this.machine.root}/`))
      throw new Error("installation read outside owned root");
    return this.remote === undefined
      ? readFile(path, "utf8")
      : this.remote.shell(`/bin/cat ${shellQuote(path)}`);
  }
  private async spawn(arguments_: readonly string[], label: string): Promise<Child> {
    const machine = this.machine;
    const name = `${machine.name}-${label}-${this.generation++}`;
    const environment = {
      TMPDIR: machine.temporaryParent,
      CODEX_ROUTER_DEBUG_APP_SERVER_SOCKET: `${machine.root}/n/app-server.sock`,
      CODEX_HOME: machine.codexHome,
      RUST_LOG: "off",
      DIRENV_DISABLE: "1",
      ...(this.input.stagingHome === undefined ? {} : { HOME: this.input.stagingHome }),
    };
    let owned: Child;
    if (this.remote !== undefined)
      owned = {
        kind: "ssh",
        process: await this.remote.start(
          [this.remote.pathFor(machine.router), ...arguments_],
          name,
          environment,
        ),
      };
    else {
      const inherited: NodeJS.ProcessEnv = { ...process.env, ...environment };
      delete inherited.CODEX_ROUTER_HOST_INHERITED_LOCK;
      delete inherited.CODEX_ROUTER_USE_HOME_DEFAULT;
      owned = {
        kind: "local",
        process: new OwnedProcess({
          executable: machine.router.path,
          arguments: arguments_,
          environment: inherited,
          cwd: this.input.repository,
          name,
          receipt: this.input.receipt,
        }),
      };
    }
    this.children.add(owned);
    return owned;
  }
  private async settle(child: Child): Promise<void> {
    if (child.kind === "local") await child.process.stop();
    else {
      if (this.remote === undefined) throw new Error("remote process has no owner");
      await this.remote.stop(child.process);
    }
    this.children.delete(child);
    if (
      (this.state.kind === "ready" || this.state.kind === "hostStarting") &&
      this.state.host === child
    )
      await this.guard.endHostWindow(this.input.receipt);
  }
  private async awaitReady(
    probe: () => Promise<void>,
    stage: () => InstallationStage,
  ): Promise<void> {
    const deadline = Date.now() + 45_000;
    let lastProbe: InstallationFailure | undefined,
      attempts = 0;
    while (Date.now() < deadline) {
      try {
        attempts += 1;
        await probe();
        return;
      } catch (error: unknown) {
        lastProbe = sanitizedInstallationFailure(stage(), error);
        await setTimeout(250);
      }
    }
    await this.input.receipt.record("installationReadinessFailed", {
      budgetMilliseconds: 45_000,
      attempts,
      lastProbe: lastProbe ?? null,
    });
    throw new Error("installation readiness deadline exceeded; no mutation replayed");
  }
  private async health(): Promise<void> {
    const url = `http://127.0.0.1:${this.machine.port}/healthz`;
    let value: unknown;
    if (this.remote !== undefined)
      value = await this.remote.execute([
        "/usr/bin/curl",
        "--silent",
        "--show-error",
        "--fail",
        "--noproxy",
        "*",
        "--max-time",
        "3",
        url,
      ]);
    else {
      const reply = await fetch(url, { redirect: "error", signal: AbortSignal.timeout(3000) });
      if (!reply.ok) throw new Error("Router health HTTP refusal");
      value = decode(await reply.text());
    }
    z.object({ binary_version: z.literal(this.input.productVersion) }).parse(value);
  }
  async start(): Promise<void> {
    if (this.state.kind !== "prepared") throw new Error("installation already started");
    if (this.continuation?.kind === "stopped") {
      await inspectStoppedOwnedRoot(this.continuation.declaration);
      await this.input.receipt.record("continuationHostWindowRequested", {
        originalBaseline: this.continuation.baseline,
        originalWindow: this.continuation.declaration.original.hostWindow,
        currentExecution: this.continuation.declaration.execution,
        controller: this.continuation.declaration.controller,
      });
    } else if (this.continuation !== undefined)
      await assertStoppedFileIdentities(this.continuation.declaration);
    if (
      this.remote !== undefined &&
      (await this.remote.listenerAddresses(this.machine.port)).length > 0
    )
      throw new Error("remote Router port collision");
    if (this.remote === undefined) {
      const probe = createServer();
      await new Promise<void>((resolve, reject): void => {
        probe.once("error", reject);
        probe.listen(this.machine.port, "127.0.0.1", resolve);
      });
      await new Promise<void>((resolve, reject): void => {
        probe.close((error): void => (error === undefined ? resolve() : reject(error)));
      });
    }
    try {
      const root = this.machine.root;
      const router = await this.spawn(
        [
          "serve",
          "--port",
          String(this.machine.port),
          "--state-db",
          installationProxyDatabase(root, this.input.productVersion),
          "--secret-root",
          `${root}/secrets`,
          "--require-debug-isolation",
          "--disable-background-quota-refresh",
        ],
        "router",
      );
      this.state = { kind: "routerRunning", router };
      await this.awaitReady(
        (): Promise<void> => this.health(),
        () => "routerHealth",
      );
      await this.startHost();
      if (this.continuation !== undefined) {
        await assertStoppedFileIdentities(this.continuation.declaration);
        const status = await this.runRouterCommand([
          "fleet",
          "status",
          "--router-root",
          this.machine.root,
        ]);
        if (status.exit !== 0) throw new Error("continued root enrollment status unavailable");
        validateUpgradeEnrollment({
          expected: this.continuation.baseline,
          identity: await this.identity(),
          status: decode(status.stdout),
          enrollmentSha256: await fingerprint(`${this.machine.root}/fleet/enrollment.json`),
        });
        if (
          (await enrollInvocationsSince(
            this.machine.root,
            this.continuation.baseline.baselineId,
          )) !== 0
        )
          throw new Error("enrollment occurred after original baseline marker");
        this.continuation = { ...this.continuation, kind: "owned" };
        await this.input.receipt.record("continuationOwnedChildrenReady", {
          originalBaseline: this.continuation.baseline,
          currentExecution: {
            router: this.machine.router,
            cli: this.machine.cli,
            productVersion: this.input.productVersion,
          },
          fileIdentityDeclaration: this.continuation.declaration.files,
          originalPidsAdopted: false,
        });
      }
    } catch (error: unknown) {
      await this.close();
      throw error;
    }
  }
  async startHost(): Promise<void> {
    if (this.state.kind !== "routerRunning")
      throw new Error("Router must be running without a Host");
    const router = this.state.router;
    await this.guard.beginHostWindow(this.input.receipt);
    await this.input.receipt.record("hostLaunchRequested", { machine: this.machine.name });
    const host = await this.spawn(
      [
        "host",
        "--router-root",
        this.machine.root,
        "--port",
        String(this.machine.port),
        "--mcp-bind",
        "127.0.0.1:0",
        "--require-debug-isolation",
      ],
      "host",
    );
    await this.input.receipt.record("hostProcessStarted", {
      machine: this.machine.name,
      pid: host.process.pid,
    });
    this.state = { kind: "hostStarting", router, host };
    let client: RouterMcp | undefined;
    let stage: InstallationStage = "hostManifestRead";
    await this.awaitReady(
      async (): Promise<void> => {
        stage = "hostManifestRead";
        const manifestValue = decode(
          await this.read(`${this.machine.root}/agent-communication/service.json`),
        );
        stage = "hostManifestShape";
        const manifest = z
          .object({
            version: z.literal(3n),
            serviceVersion: z.literal(this.input.productVersion),
            routerProxyEndpoint: z.literal(`127.0.0.1:${this.machine.port}`),
            mcp: z.object({ url: z.string() }),
          })
          .parse(manifestValue);
        stage = "hostMcpTarget";
        const candidate = new RouterMcp(manifest.mcp.url, this.remote);
        stage = "hostEndpointRead";
        const endpointReply = await candidate.call("endpoints_list", {}, 5);
        stage = "hostEndpointShape";
        const inventory = z
          .object({
            endpoints: z.array(
              z.object({
                endpoint: z.object({ endpointId: z.string() }),
                availability: z.object({ state: z.string() }),
              }),
            ),
          })
          .parse(endpointReply);
        stage = "hostNativeAvailability";
        if (
          !inventory.endpoints.some(
            (item): boolean =>
              item.endpoint.endpointId === "codex-local" && item.availability.state === "available",
          )
        )
          throw new Error("native endpoint not available yet");
        client = candidate;
      },
      () => stage,
    );
    if (client === undefined) throw new Error("ready Host omitted MCP client");
    this.state = { kind: "ready", router, host, mcp: client };
    await this.check();
  }
  async identity(): Promise<InstallationIdentity> {
    return identitySchema.parse(await this.mcp.call("installation_show", {}));
  }
  async runRouterCommand(
    arguments_: readonly string[],
    environment: Readonly<Record<string, string>> = {},
  ): Promise<Awaited<ReturnType<typeof command>>> {
    if (this.state.kind === "closed") throw new Error("closed installation cannot run commands");
    const childEnvironment: NodeJS.ProcessEnv = {
      ...process.env,
      ...environment,
      CODEX_HOME: this.machine.codexHome,
      ...(this.input.stagingHome === undefined ? {} : { HOME: this.input.stagingHome }),
    };
    delete childEnvironment.CODEX_ROUTER_HOST_INHERITED_LOCK;
    delete childEnvironment.CODEX_ROUTER_USE_HOME_DEFAULT;
    if (this.remote !== undefined)
      return this.remote.output([this.remote.pathFor(this.machine.router), ...arguments_], {
        environment,
        timeoutSeconds: 60,
      });
    return command(this.machine.router.path, arguments_, {
      environment: childEnvironment,
      timeoutSeconds: 60,
      cwd: this.input.repository,
    });
  }
  async markUpgradeBaseline(): Promise<UpgradeBaseline> {
    if (this.remote !== undefined || this.state.kind !== "ready")
      throw new Error(
        "upgrade baseline requires a ready local owned installation; run each Mac's launcher locally",
      );
    if (this.continuation !== undefined) {
      await this.verifyOriginalEnrollment(this.continuation.baseline);
      return structuredClone(this.continuation.baseline);
    }
    const identity = await this.identity();
    const result = await this.runRouterCommand([
      "fleet",
      "status",
      "--router-root",
      this.machine.root,
    ]);
    if (result.exit !== 0) throw new Error("existing enrollment status unavailable");
    const enrolled = enrollmentStatus.parse(decode(result.stdout)).status.enrollment;
    if (enrolled.peer !== identity.peerId)
      throw new Error("enrolled peer differs from running installation");
    const baselineId = await markCommandBaseline(this.machine.root);
    const baseline = upgradeBaselineSchema.parse({
      root: this.machine.root,
      installationId: identity.serviceId,
      machineId: identity.peerId,
      endpoint: enrolled.endpoint,
      enrollmentSha256: await fingerprint(`${this.machine.root}/fleet/enrollment.json`),
      baselineId,
    });
    await this.input.receipt.save(`upgrade-baseline-${baselineId}.json`, baseline);
    return baseline;
  }
  private async verifyOriginalEnrollment(baseline: UpgradeBaseline): Promise<void> {
    const status = await this.runRouterCommand([
      "fleet",
      "status",
      "--router-root",
      this.machine.root,
    ]);
    if (status.exit !== 0) throw new Error("original enrollment status unavailable");
    validateUpgradeEnrollment({
      expected: baseline,
      identity: await this.identity(),
      status: decode(status.stdout),
      enrollmentSha256: await fingerprint(`${this.machine.root}/fleet/enrollment.json`),
    });
    if ((await enrollInvocationsSince(this.machine.root, baseline.baselineId)) !== 0)
      throw new Error("enrollment occurred after original baseline marker");
  }
  async upgrade(_input: unknown): Promise<never> {
    throw new Error(
      "retired: the updater was replaced by scripts/one-off/2026-11-convert-router-root.ts",
    );
  }
  async stopHost(): Promise<void> {
    if (this.state.kind !== "ready") throw new Error("no ready Host to stop");
    const { host, router } = this.state;
    await this.settle(host);
    this.state = { kind: "routerRunning", router };
    await this.guard.verify(this.input.receipt);
  }
  async check(): Promise<void> {
    await this.health();
    if (this.remote !== undefined) await this.remote.budget({ kind: "running" });
    for (const artifact of [this.machine.router, this.machine.cli]) {
      const hash =
        this.remote === undefined
          ? await fingerprint(artifact.path)
          : (
              await this.remote.shell(
                `/usr/bin/shasum -a 256 ${shellQuote(this.remote.pathFor(artifact))}`,
              )
            )
              .trim()
              .split(/\s+/)[0];
      if (hash !== artifact.sha256) throw new Error("installation artifact changed during run");
    }
    const ownedPids: number[] = [];
    for (const child of this.children) {
      if (child.kind === "ssh") {
        if (this.remote === undefined) throw new Error("SSH owner absent");
        ownedPids.push(...(await this.remote.capture(child.process)));
      } else {
        const observed = await command(
          "/bin/ps",
          ["-p", String(child.process.pid), "-o", "pid=,lstart=,comm="],
          { cwd: this.machine.root },
        );
        if (observed.stdout !== child.process.identity)
          throw new Error("owned installation PID changed or ended");
        ownedPids.push(...child.process.capture());
      }
    }
    await this.guard.verify(this.input.receipt, [...new Set(ownedPids)]);
    if (
      this.input.stagingHome !== undefined &&
      (this.state.kind === "ready" || this.state.kind === "hostStarting")
    ) {
      const observation = await observeRuntimeHomeIsolation({
        hostPid: this.state.host.process.pid,
        ownedPids: [...new Set(ownedPids)],
        expectedHome: this.input.stagingHome,
        expectedCodexHome: this.machine.codexHome,
        root: this.machine.root,
      });
      await this.input.receipt.record("runtimeHomeIsolation", observation);
      if (observation.forbiddenFiles.length !== 0)
        throw new Error("owned Host or child opened a real Codex/Claude home path");
    }
  }
  async close(): Promise<void> {
    const failures: unknown[] = [];
    for (const child of [...this.children].reverse()) {
      try {
        await this.settle(child);
      } catch (error: unknown) {
        failures.push(error);
      }
    }
    this.state = { kind: "closed" };
    try {
      await this.guard.endHostWindow(this.input.receipt);
      await this.guard.verify(this.input.receipt);
    } catch (error: unknown) {
      failures.push(error);
    }
    if (failures.length > 0)
      throw new AggregateError(failures, "installation cleanup or configuration unverified");
    this.releaseCommandRoot();
  }
}
