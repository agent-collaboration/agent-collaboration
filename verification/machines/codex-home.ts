import { createHash } from "node:crypto";
import {
  readFile,
  realpath,
  stat,
  lstat,
  readdir,
  mkdir,
  writeFile,
  copyFile,
  chmod,
} from "node:fs/promises";
import { constants } from "node:fs";
import { isDeepStrictEqual } from "node:util";
import { parse } from "smol-toml";
import { z } from "zod";
import { userInfo } from "node:os";
import { resolve } from "node:path";
import type { Machine, Artifact } from "../declaration.ts";
import { ObservationOpen, type Receipt } from "../receipt.ts";
import type { Remote } from "./remote.ts";
import { shellQuote, privateDirectory, qualifyArtifact, fingerprint } from "./owned-process.ts";
import { sampleConfigAccess } from "./config-access.ts";
import {
  hashRealClaudeRegistry,
  privateTreeSchema,
  snapshotExistingPrivateTree,
  snapshotExistingStagingUserHome,
} from "./user-home-isolation.ts";

const files = ["config.toml", "codex-router-debug.config.toml"];
interface GuardSnapshot {
  readonly hashes: readonly string[];
  readonly modifiedAt: readonly string[];
}
export interface ConfigurationEligibility {
  readonly eligible: boolean;
  readonly requiredFilesReadable: boolean;
  readonly ownedWriteAccessDuringChange: boolean;
}
export type CodexHomePurpose = "normal" | "staging-isolated";
export const existingStagingHomesSchema = z
  .object({
    home: z.string(),
    codexHome: z.string(),
    nativeCwd: z.string(),
    privateHomeTree: privateTreeSchema,
    codexHomeTree: privateTreeSchema,
  })
  .strict();
export type ExistingStagingHomes = z.infer<typeof existingStagingHomesSchema>;
const existingCodexEntries = [
  ".sqlite-maintenance.lock",
  ".tmp",
  "app-server-control",
  "codex-router-debug.config.toml",
  "config.toml",
  "goals_1.sqlite",
  "installation_id",
  "logs_2.sqlite",
  "memories_1.sqlite",
  "packages",
  "queue_1.sqlite",
  "skills",
  "state_5.sqlite",
] as const;
const nativeSqliteSidecars = [
  "goals_1.sqlite",
  "logs_2.sqlite",
  "memories_1.sqlite",
  "queue_1.sqlite",
  "state_5.sqlite",
].flatMap((name) => [`${name}-wal`, `${name}-shm`]);
async function validateNativeSqliteSidecars(directory: string): Promise<void> {
  for (const name of await readdir(directory)) {
    if (!nativeSqliteSidecars.includes(name)) continue;
    const info = await lstat(`${directory}/${name}`);
    if (
      !info.isFile() ||
      info.isSymbolicLink() ||
      info.uid !== process.getuid?.() ||
      (info.mode & 0o777) !== 0o600 ||
      (await realpath(`${directory}/${name}`)) !== `${directory}/${name}`
    )
      throw new Error("known regular owner-private SQLite sidecar required");
  }
}
function stagingConfiguration(machine: Machine): { config: string; profile: string } {
  const trust = `[projects.${JSON.stringify(machine.nativeCwd)}]\ntrust_level = "trusted"\n`;
  return {
    config: trust,
    profile: `model_provider = "codex-router-debug"\n${trust}\n[model_providers.codex-router-debug]\nname = "OpenAI"\nbase_url = "http://127.0.0.1:${machine.port}/v1"\nwire_api = "responses"\nrequires_openai_auth = false\nsupports_websockets = true\n\n[features]\nenable_request_compression = false\n`,
  };
}
function checkStagingHomePath(machine: Machine): void {
  if (
    machine.kind !== "local" ||
    resolve(machine.codexHome) !== machine.codexHome ||
    !machine.codexHome.startsWith(`${machine.temporaryParent}/`) ||
    machine.codexHome === machine.root ||
    machine.codexHome.startsWith(`${machine.root}/`) ||
    /(^|\/)\.codex(\/|$)/.test(machine.codexHome)
  )
    throw new Error("separate staging-only Codex home beneath allocated private parent required");
}
export async function validateStagingCodexHome(machine: Machine): Promise<void> {
  checkStagingHomePath(machine);
  await privateDirectory(machine.temporaryParent);
  await privateDirectory(machine.codexHome);
  const allowed = new Set(["config.toml", "codex-router-debug.config.toml", "packages"]);
  for (const item of await readdir(machine.codexHome))
    if (!allowed.has(item))
      throw new Error(
        "staging Codex home contains state, sessions, auth or other unapproved input",
      );
  await validateStagingConfigurationFiles(machine);
}

async function validateStagingConfigurationFiles(machine: Machine): Promise<void> {
  const expected = stagingConfiguration(machine);
  for (const [name, content] of [
    ["config.toml", expected.config],
    ["codex-router-debug.config.toml", expected.profile],
  ]) {
    if (name === undefined || content === undefined)
      throw new Error("staging configuration pair required");
    const path = `${machine.codexHome}/${name}`,
      info = await lstat(path);
    if (
      !info.isFile() ||
      info.isSymbolicLink() ||
      info.uid !== process.getuid?.() ||
      (info.mode & 0o777) !== 0o600 ||
      (await realpath(path)) !== path ||
      (await readFile(path, "utf8")) !== content
    )
      throw new Error(
        "fresh minimal staging configuration required; production configuration is not copied",
      );
  }
}

/** Existing stopped private state is inventoried exactly; fresh-root guards retain their stricter contract. */
export async function snapshotExistingStagingHomes(input: {
  machine: Machine;
  home: string;
}): Promise<ExistingStagingHomes> {
  checkStagingHomePath(input.machine);
  await privateDirectory(input.machine.codexHome);
  await validateStagingConfigurationFiles(input.machine);
  const privateHomeTree = await snapshotExistingStagingUserHome(input);
  await validateNativeSqliteSidecars(input.machine.codexHome);
  const codexHomeTree = await snapshotExistingPrivateTree(input.machine.codexHome, [
    ...existingCodexEntries,
    ...nativeSqliteSidecars,
  ]);
  return {
    home: input.home,
    codexHome: input.machine.codexHome,
    nativeCwd: input.machine.nativeCwd,
    privateHomeTree,
    codexHomeTree,
  };
}

export async function validateExistingStagingHomes(input: {
  machine: Machine;
  home: string;
  expected: ExistingStagingHomes;
}): Promise<void> {
  const expected = existingStagingHomesSchema.parse(input.expected);
  const actual = await snapshotExistingStagingHomes(input);
  if (!isDeepStrictEqual(actual, expected))
    throw new Error("existing private-home declaration changed before continuation");
}
export async function prepareStagingCodexHome(input: {
  machine: Machine;
  managedCodex: Artifact;
}): Promise<void> {
  const { machine } = input;
  checkStagingHomePath(machine);
  await privateDirectory(machine.temporaryParent);
  await qualifyArtifact(input.managedCodex);
  await mkdir(machine.codexHome, { mode: 0o700 });
  const configuration = stagingConfiguration(machine);
  await writeFile(`${machine.codexHome}/config.toml`, configuration.config, {
    mode: 0o600,
    flag: "wx",
  });
  await writeFile(`${machine.codexHome}/codex-router-debug.config.toml`, configuration.profile, {
    mode: 0o600,
    flag: "wx",
  });
  const directory = `${machine.codexHome}/packages/standalone/current`;
  await mkdir(directory, { recursive: true, mode: 0o700 });
  const executable = `${directory}/codex`;
  await copyFile(input.managedCodex.path, executable, constants.COPYFILE_EXCL);
  await chmod(executable, 0o700);
  if ((await fingerprint(executable)) !== input.managedCodex.sha256)
    throw new Error("managed Codex artifact copy differs");
  await qualifyArtifact({ ...input.managedCodex, path: executable });
  await validateStagingCodexHome(machine);
}
export class CodexHomeGuard {
  private readonly machine: Machine;
  private readonly remote: Remote | undefined;
  private readonly before: GuardSnapshot;
  private hostWindow: GuardSnapshot | undefined;
  private requiredFilesReadable = true;
  private ownedWriteAccessDuringChange = false;
  private userIsolation:
    | { kind: "normal" }
    | { kind: "staging-isolated"; claudeRegistrySha256: string; allocatedMachine: Machine } = {
    kind: "normal",
  };
  private constructor(machine: Machine, remote: Remote | undefined, before: GuardSnapshot) {
    this.machine = machine;
    this.remote = remote;
    this.before = before;
  }
  private async contents(name: string): Promise<string> {
    if (!files.includes(name)) throw new Error("configuration guard file outside declared pair");
    const path = `${this.machine.codexHome}/${name}`;
    return this.remote === undefined
      ? readFile(path, "utf8")
      : this.remote.shell(`/bin/cat ${shellQuote(path)}`);
  }
  get eligibility(): ConfigurationEligibility {
    return {
      eligible: this.requiredFilesReadable && !this.ownedWriteAccessDuringChange,
      requiredFilesReadable: this.requiredFilesReadable,
      ownedWriteAccessDuringChange: this.ownedWriteAccessDuringChange,
    };
  }
  private async snapshot(): Promise<GuardSnapshot> {
    const snapshots = await Promise.all(
      files.map(async (name) => {
        const path = `${this.machine.codexHome}/${name}`;
        try {
          const hash = createHash("sha256")
            .update(await this.contents(name))
            .digest("hex");
          const time =
            this.remote === undefined
              ? (await stat(path)).mtime
              : new Date(
                  z.coerce
                    .number()
                    .int()
                    .positive()
                    .parse(
                      (await this.remote.shell(`/usr/bin/stat -f %m ${shellQuote(path)}`)).trim(),
                    ) * 1000,
                );
          return { hash, modifiedAt: time.toISOString() };
        } catch (cause: unknown) {
          this.requiredFilesReadable = false;
          throw new ObservationOpen("required Codex configuration could not be read or hashed", {
            cause,
          });
        }
      }),
    );
    return {
      hashes: snapshots.map((row) => row.hash),
      modifiedAt: snapshots.map((row) => row.modifiedAt),
    };
  }
  static async capture(
    machine: Machine,
    remote?: Remote,
    purpose: CodexHomePurpose = "normal",
  ): Promise<CodexHomeGuard> {
    const normalHome =
      remote === undefined
        ? `${userInfo().homedir}/.codex`
        : (await remote.shell("printf '%s/.codex' \"$HOME\"")).trim();
    if (purpose === "staging-isolated") {
      if (remote !== undefined)
        throw new Error("staging isolated Host must run through its local named agent");
      await validateStagingCodexHome(machine);
    } else if (machine.codexHome !== normalHome) throw new Error("normal Codex home required");
    return CodexHomeGuard.captureValidated({ machine, remote, purpose, normalHome });
  }
  static async captureExistingStaging(input: {
    machine: Machine;
    home: string;
    expected: ExistingStagingHomes;
  }): Promise<CodexHomeGuard> {
    await validateExistingStagingHomes(input);
    return CodexHomeGuard.captureValidated({
      machine: input.machine,
      remote: undefined,
      purpose: "staging-isolated",
      normalHome: `${userInfo().homedir}/.codex`,
    });
  }
  private static async captureValidated(input: {
    machine: Machine;
    remote: Remote | undefined;
    purpose: CodexHomePurpose;
    normalHome: string;
  }): Promise<CodexHomeGuard> {
    const { machine, remote, purpose, normalHome } = input;
    const guardedMachine = { ...machine, codexHome: normalHome };
    const guard = new CodexHomeGuard(guardedMachine, remote, { hashes: [], modifiedAt: [] });
    const cwd =
      remote === undefined
        ? await realpath(machine.nativeCwd)
        : (await remote.shell(`cd ${shellQuote(machine.nativeCwd)} && /bin/pwd -P`)).trim();
    if (cwd !== machine.nativeCwd || (remote === undefined && !(await stat(cwd)).isDirectory()))
      throw new Error("native cwd must be an existing canonical directory");
    const before = await guard.snapshot();
    const config = z
      .record(z.string(), z.unknown())
      .parse(
        parse(
          purpose === "staging-isolated"
            ? await readFile(`${machine.codexHome}/config.toml`, "utf8")
            : await guard.contents("config.toml"),
        ),
      );
    const projects = z.record(z.string(), z.unknown()).parse(config.projects ?? {});
    if (!z.object({ trust_level: z.literal("trusted") }).safeParse(projects[cwd]).success)
      throw new Error(
        "native cwd lacks an existing exact trust entry; configuration left untouched",
      );
    const ready = new CodexHomeGuard(guardedMachine, remote, before);
    if (purpose === "staging-isolated")
      ready.userIsolation = {
        kind: "staging-isolated",
        claudeRegistrySha256: await hashRealClaudeRegistry(),
        allocatedMachine: machine,
      };
    await ready.verify();
    return ready;
  }
  private async verifySnapshot(
    after: GuardSnapshot,
    receipt?: Receipt,
    ownedPids: readonly number[] = [],
  ): Promise<void> {
    if (this.userIsolation.kind === "staging-isolated")
      await validateStagingConfigurationFiles(this.userIsolation.allocatedMachine);
    const baseline = {
      hashes: [this.before.hashes[0], this.hostWindow?.hashes[1] ?? this.before.hashes[1]],
      modifiedAt: [
        this.before.modifiedAt[0],
        this.hostWindow?.modifiedAt[1] ?? this.before.modifiedAt[1],
      ],
    };
    const unchanged =
      baseline.hashes.length === after.hashes.length &&
      baseline.hashes.every((hash, index): boolean => hash === after.hashes[index]);
    const ownedFileAccess = await sampleConfigAccess({
      remote: this.remote,
      paths: files.map((name) => `${this.machine.codexHome}/${name}`),
      ownedPids,
    });
    // A changed file held writable by an owned process cannot establish the
    // read-only configuration boundary. This is correlation, not syscall tracing.
    if (
      ownedFileAccess.openFiles.some(
        (file) =>
          (file.access === "w" || file.access === "u") &&
          baseline.hashes[file.file] !== after.hashes[file.file],
      )
    )
      this.ownedWriteAccessDuringChange = true;
    if (receipt !== undefined)
      await receipt.record("codexHomeGuard", {
        machine: this.machine.name,
        unchanged,
        debugProfileGuarded: this.hostWindow !== undefined,
        before: baseline.hashes,
        after: after.hashes,
        beforeModifiedAt: baseline.modifiedAt,
        afterModifiedAt: after.modifiedAt,
        stopsCases: false,
        eligibility: this.eligibility,
        ownedFileAccess,
      });
    if (this.userIsolation.kind === "staging-isolated") {
      const afterPeerRegistry = await hashRealClaudeRegistry();
      const peersUnchanged = afterPeerRegistry === this.userIsolation.claudeRegistrySha256;
      if (receipt !== undefined)
        await receipt.record("realUserIsolationGuard", {
          machine: this.machine.name,
          codexConfigurationUnchanged: unchanged,
          claudeRegistryUnchanged: peersUnchanged,
          claudeRegistryBeforeSha256: this.userIsolation.claudeRegistrySha256,
          claudeRegistryAfterSha256: afterPeerRegistry,
        });
      if (!unchanged)
        throw new ObservationOpen(
          "real Codex configuration changed during staging isolation window",
        );
    }
  }
  async beginHostWindow(receipt: Receipt): Promise<void> {
    if (this.hostWindow !== undefined) throw new Error("Host guard window already active");
    const baseline = await this.snapshot();
    await this.verifySnapshot(baseline, receipt);
    this.hostWindow = baseline;
    await receipt.record("debugProfileWindowStarted", {
      machine: this.machine.name,
      changedSincePreparation: this.before.hashes[1] !== baseline.hashes[1],
      sha256: baseline.hashes[1],
      modifiedAt: baseline.modifiedAt[1],
    });
  }
  async endHostWindow(receipt: Receipt): Promise<void> {
    if (this.hostWindow === undefined) return;
    try {
      await this.verify(receipt);
      await receipt.record("debugProfileWindowEnded", { machine: this.machine.name });
    } finally {
      this.hostWindow = undefined;
    }
  }
  async verify(receipt?: Receipt, ownedPids: readonly number[] = []): Promise<void> {
    await this.verifySnapshot(await this.snapshot(), receipt, ownedPids);
  }
}
