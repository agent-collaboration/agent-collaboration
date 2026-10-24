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
import { CodexHomeGuard } from "./codex-home.ts";

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
const identitySchema = z.object({
  serviceId: z.uuid(),
  machineLabel: z.string(),
  peerId: z.string().regex(/^[a-f0-9]{8}-[a-f0-9]{4}-7[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/),
});
export type InstallationIdentity = z.infer<typeof identitySchema>;
export class Installation {
  readonly machine: Machine;
  readonly remote: Remote | undefined;
  readonly cli: RouterCli;
  private readonly input: { repository: string; productVersion: string; receipt: Receipt };
  private readonly guard: CodexHomeGuard;
  private readonly children = new Set<Child>();
  private state: State = { kind: "prepared" };
  private generation = 0;
  private constructor(input: {
    machine: Machine;
    remote: Remote | undefined;
    guard: CodexHomeGuard;
    repository: string;
    productVersion: string;
    receipt: Receipt;
  }) {
    this.machine = input.machine;
    this.remote = input.remote;
    this.guard = input.guard;
    this.input = input;
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
    const guard = await CodexHomeGuard.capture(machine, remote);
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
    const providers = encode({
      version: 1,
      providers: {
        claude: { enabled: false, executable: null, arguments: [] },
        cursor: { enabled: false, executable: null, arguments: [] },
      },
    });
    if (remote === undefined)
      await writeFile(`${machine.root}/providers.json`, providers, { mode: 0o600, flag: "wx" });
    else await remote.writePrivate("providers.json", providers);
    return new Installation({ ...input, remote, guard });
  }
  get mcp(): RouterMcp {
    if (this.state.kind !== "ready") throw new Error("installation Host is not ready");
    return this.state.mcp;
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
  private async awaitReady(probe: () => Promise<void>): Promise<void> {
    const deadline = Date.now() + 45_000;
    while (Date.now() < deadline) {
      try {
        await probe();
        return;
      } catch {
        await setTimeout(250);
      }
    }
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
          `${root}/state.sqlite`,
          "--secret-root",
          `${root}/secrets`,
          "--require-debug-isolation",
          "--disable-background-quota-refresh",
        ],
        "router",
      );
      this.state = { kind: "routerRunning", router };
      await this.awaitReady((): Promise<void> => this.health());
      await this.startHost();
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
    await this.awaitReady(async (): Promise<void> => {
      const manifest = z
        .object({
          version: z.literal(3n),
          serviceVersion: z.literal(this.input.productVersion),
          routerProxyEndpoint: z.literal(`127.0.0.1:${this.machine.port}`),
          mcp: z.object({ url: z.string() }),
        })
        .parse(decode(await this.read(`${this.machine.root}/agent-communication/service.json`)));
      const candidate = new RouterMcp(manifest.mcp.url, this.remote);
      const inventory = z
        .object({
          endpoints: z.array(
            z.object({
              endpoint: z.object({ endpointId: z.string() }),
              availability: z.object({ state: z.string() }),
            }),
          ),
        })
        .parse(await candidate.call("endpoints_list", {}, 5));
      if (
        !inventory.endpoints.some(
          (item): boolean =>
            item.endpoint.endpointId === "codex-local" && item.availability.state === "available",
        )
      )
        throw new Error("native endpoint not available yet");
      client = candidate;
    });
    if (client === undefined) throw new Error("ready Host omitted MCP client");
    this.state = { kind: "ready", router, host, mcp: client };
    await this.check();
  }
  async identity(): Promise<InstallationIdentity> {
    return identitySchema.parse(await this.mcp.call("installation_show", {}));
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
        const observed = await command("/bin/ps", [
          "-p",
          String(child.process.pid),
          "-o",
          "pid=,lstart=,comm=",
        ]);
        if (observed.stdout !== child.process.identity)
          throw new Error("owned installation PID changed or ended");
        ownedPids.push(...child.process.capture());
      }
    }
    await this.guard.verify(this.input.receipt, [...new Set(ownedPids)]);
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
  }
}
