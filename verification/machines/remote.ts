import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import { z } from "zod";
import { resolve, basename } from "node:path";
import type { Artifact, Machine } from "../declaration.ts";
import { environmentName } from "../declaration.ts";
import { decode } from "../exact-json.ts";
import { command, shellQuote, qualifyArtifact } from "./owned-process.ts";

export interface RemoteProcess {
  readonly pid: number;
  readonly identity: string;
  readonly descendants: Map<number, string>;
}

export class Remote {
  readonly machine: Extract<Machine, { kind: "ssh" }>;
  readonly sshArguments: readonly string[];
  constructor(machine: Extract<Machine, { kind: "ssh" }>) {
    if (
      resolve(machine.root) !== machine.root ||
      !machine.root.startsWith(`${machine.temporaryParent}/`) ||
      /(^|\/)\.(codex|codex-router|claude)(\/|$)/.test(machine.root) ||
      !machine.retainedRoots.includes(machine.root)
    )
      throw new Error("remote root is not isolated or retained in budget");
    this.machine = machine;
    this.sshArguments = ["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", machine.host];
  }
  async shell(script: string, seconds = 30): Promise<string> {
    const result = await command("/usr/bin/ssh", [...this.sshArguments, script], {
      timeoutSeconds: seconds,
    });
    if (result.exit !== 0) throw new Error("bounded SSH command failed; no mutation replayed");
    return result.stdout;
  }
  async json(script: string): Promise<unknown> {
    return decode(await this.shell(script));
  }
  async execute(arguments_: readonly string[]): Promise<unknown> {
    return decode(await this.shell(arguments_.map(shellQuote).join(" ")));
  }
  async output(
    arguments_: readonly string[],
    options: { timeoutSeconds?: number; environment?: Readonly<Record<string, string>> } = {},
  ): Promise<{ stdout: string; stderr: string; exit: number | null }> {
    const environment = Object.entries(options.environment ?? {});
    const exports = environment.map(([name, value], index): string => {
      environmentName.parse(name);
      if (/[\r\n\0]/.test(value))
        throw new Error("remote environment value cannot contain a line break");
      return `IFS= read -r harness_value_${index}; export ${name}="$harness_value_${index}"`;
    });
    const script = [...exports, `exec ${arguments_.map(shellQuote).join(" ")}`].join("; ");
    return command("/usr/bin/ssh", [...this.sshArguments, `set -eu; ${script}`], {
      timeoutSeconds: options.timeoutSeconds ?? 30,
      ...(environment.length === 0
        ? {}
        : { input: `${environment.map(([, value]): string => value).join("\n")}\n` }),
    });
  }
  async writePrivate(relativePath: string, contents: string): Promise<void> {
    if (!/^[a-z0-9][a-z0-9./-]*$/.test(relativePath) || relativePath.split("/").includes(".."))
      throw new Error("private file must be inside the owned root");
    const path = shellQuote(`${this.machine.root}/${relativePath}`);
    const sha256 = createHash("sha256").update(contents).digest("hex");
    const ack = shellQuote(JSON.stringify({ kind: "privateFileWritten", sha256 }));
    const script = `set -eu; umask 077; test ! -e ${path}; test ! -L ${path}; cat > ${path}; chmod 600 ${path}; test $(/usr/bin/shasum -a 256 ${path} | awk '{print $1}') = ${sha256}; printf '%s' ${ack}`;
    const result = await command("/usr/bin/ssh", [...this.sshArguments, script], {
      input: contents,
    });
    if (result.exit !== 0) throw new Error("private file write unconfirmed; no replay");
    z.object({ kind: z.literal("privateFileWritten"), sha256: z.literal(sha256) }).parse(
      decode(result.stdout),
    );
  }
  pathFor(artifact: Artifact): string {
    return `${this.machine.root}/bin/${basename(artifact.path)}`;
  }
  async listenerAddresses(port: number): Promise<readonly string[]> {
    z.number()
      .int()
      .min(1024)
      .max(65535)
      .refine((value): boolean => ![8787, 8788, 43127].includes(value))
      .parse(port);
    const table = await this.shell("/usr/sbin/netstat -an -p tcp");
    if (!/Proto\s+Recv-Q\s+Send-Q\s+Local Address/.test(table))
      throw new Error("remote socket table format unknown");
    return table
      .split("\n")
      .map((line): string[] => line.trim().split(/\s+/))
      .filter(
        (fields): boolean =>
          fields[0]?.startsWith("tcp") === true &&
          fields.at(-1) === "LISTEN" &&
          fields[3]?.endsWith(`.${port}`) === true,
      )
      .map((fields): string => fields[3] ?? "");
  }
  async budget(
    input: { kind: "transfer"; additionalBytes: number } | { kind: "running" } = {
      kind: "transfer",
      additionalBytes: 0,
    },
  ): Promise<void> {
    const free = Number((await this.shell("df -k / | awk 'NR==2{print $4}'")).trim());
    const retained = (
      await this.shell(
        this.machine.retainedRoots
          .map(
            (root): string =>
              `if test -e ${shellQuote(root)}; then du -sk ${shellQuote(root)}; else printf '0 missing\\n'; fi`,
          )
          .join("; "),
      )
    )
      .trim()
      .split("\n")
      .reduce((sum, line): number => sum + z.coerce.number().parse(line.split(/\s+/)[0]), 0);
    const incoming =
      input.kind === "transfer"
        ? Math.ceil(z.number().nonnegative().parse(input.additionalBytes) / 4096) * 4
        : 0;
    const minimumFree = (input.kind === "transfer" ? 20 : 15) * 1024 * 1024;
    if (!Number.isFinite(free) || free < minimumFree || retained + incoming > 5 * 1024 * 1024)
      throw new Error(
        `Sunclaw budget refused: freeKiB=${free}, retainedKiB=${retained}, incomingKiB=${incoming}`,
      );
  }
  async prepare(): Promise<void> {
    await this.budget();
    const root = shellQuote(this.machine.root),
      parent = shellQuote(this.machine.temporaryParent);
    const acknowledgement = shellQuote(JSON.stringify({ kind: "privateRootCreated" }));
    const result = await this.json(
      `set -eu; test -d ${parent}; test ! -L ${parent}; test ! -e ${root}; umask 077; mkdir ${root}; mkdir ${shellQuote(`${this.machine.root}/bin`)}; test $(/usr/bin/stat -f %OLp ${root}) = 700; test $(/usr/bin/stat -f %u ${root}) = $(/usr/bin/id -u); printf '%s' ${acknowledgement}`,
    );
    z.object({ kind: z.literal("privateRootCreated") }).parse(result);
  }
  async transfer(artifact: Artifact, destination: string): Promise<void> {
    if (!destination.startsWith(`${this.machine.root}/`) || destination.includes("/../"))
      throw new Error("remote transfer outside declared root");
    await this.budget({ kind: "transfer", additionalBytes: (await stat(artifact.path)).size });
    await qualifyArtifact(artifact);
    const quoted = shellQuote(destination);
    const acknowledgement = shellQuote(
      JSON.stringify({ kind: "artifactStored", sha256: artifact.sha256 }),
    );
    const script = `set -eu; umask 077; test ! -e ${quoted}; cat > ${quoted}; chmod 700 ${quoted}; /usr/bin/codesign --verify --strict ${quoted}; /usr/bin/shasum -a 256 ${quoted}; printf '\n%s' ${acknowledgement}`;
    const child = spawn("/usr/bin/ssh", [...this.sshArguments, script], {
      stdio: ["pipe", "pipe", "pipe"],
      signal: AbortSignal.timeout(120_000),
    });
    const source = createReadStream(artifact.path);
    source.pipe(child.stdin);
    let output = "";
    child.stdout.on("data", (chunk: Buffer): void => {
      output += chunk.toString();
    });
    const exit = await new Promise<number | null>((resolve, reject): void => {
      source.once("error", (): void => {
        child.kill("SIGTERM");
        reject(new Error("artifact source read failed"));
      });
      child.stdin.once("error", (): void => {
        source.destroy();
        child.kill("SIGTERM");
        reject(new Error("artifact transfer input failed"));
      });
      child.once("error", reject);
      child.once("exit", resolve);
    });
    const lines = output.trim().split("\n");
    if (exit !== 0 || lines[0]?.split(/\s+/)[0] !== artifact.sha256)
      throw new Error("remote artifact hash mismatch or result unknown");
    z.object({ kind: z.literal("artifactStored"), sha256: z.literal(artifact.sha256) }).parse(
      decode(lines.at(-1) ?? ""),
    );
    await this.budget({ kind: "running" });
  }
  async start(
    arguments_: readonly string[],
    name: string,
    environment: Readonly<Record<string, string>> = {},
  ): Promise<RemoteProcess> {
    if (!/^[a-z][a-z0-9-]*$/.test(name)) throw new Error("remote process name invalid");
    const root = this.machine.root;
    const assignments = Object.entries(environment)
      .map(([key, value]): string => {
        if (!/^[A-Z][A-Z0-9_]*$/.test(key)) throw new Error("environment key invalid");
        return `${key}=${shellQuote(value)}`;
      })
      .join(" ");
    const pid = z.coerce
      .number()
      .int()
      .positive()
      .parse(
        (
          await this.shell(
            `umask 077; ${assignments} nohup ${arguments_.map(shellQuote).join(" ")} >${shellQuote(`${root}/${name}.stdout`)} 2>${shellQuote(`${root}/${name}.stderr`)} </dev/null & echo $!`,
          )
        ).trim(),
      );
    const identity = await this.shell(`/bin/ps -p ${pid} -o pid=,lstart=,comm=`);
    if (!identity.trim()) throw new Error("remote child identity absent");
    return { pid, identity, descendants: new Map() };
  }
  async capture(owned: RemoteProcess): Promise<readonly number[]> {
    if (
      (await this.shell(`/bin/ps -p ${owned.pid} -o pid=,lstart=,comm= || true`)) !== owned.identity
    )
      throw new Error("remote PID absent or changed; observation invalid");
    const table = (await this.shell("/bin/ps -axo pid=,ppid=,lstart=,comm=")).split("\n");
    const selected = new Set([owned.pid]);
    for (let pass = 0; pass < 20; pass += 1)
      for (const line of table) {
        const match = line.trim().match(/^(\d+)\s+(\d+)\s+(.+)$/);
        if (
          match?.[1] !== undefined &&
          match[2] !== undefined &&
          match[3] !== undefined &&
          selected.has(Number(match[2]))
        ) {
          const pid = Number(match[1]);
          selected.add(pid);
          if (!owned.descendants.has(pid)) owned.descendants.set(pid, match[3]);
        }
      }
    return [...selected];
  }
  async stop(owned: RemoteProcess): Promise<void> {
    const observed = await this.shell(`/bin/ps -p ${owned.pid} -o pid=,lstart=,comm= || true`);
    if (observed !== "" && observed !== owned.identity)
      throw new Error("remote PID changed; refusing signal");
    if (observed !== "") {
      await this.capture(owned);
      const acknowledgement = shellQuote(JSON.stringify({ kind: "ownedProcessStopped" }));
      const answer = await this.json(
        `set -eu; /bin/kill -TERM ${owned.pid}; for tick in $(/usr/bin/seq 1 100); do if ! /bin/kill -0 ${owned.pid} 2>/dev/null; then printf '%s' ${acknowledgement}; exit 0; fi; sleep 0.1; done; exit 2`,
      );
      z.object({ kind: z.literal("ownedProcessStopped") }).parse(answer);
    }
    for (const pid of owned.descendants.keys()) {
      if ((await this.shell(`/bin/ps -p ${pid} -o lstart=,comm= || true`)).trim() !== "")
        throw new Error("remote captured descendant remains; no unverified PID signalled");
    }
  }
}
