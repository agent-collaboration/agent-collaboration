import { createHash } from "node:crypto";
import { readFile, realpath, stat } from "node:fs/promises";
import { parse } from "smol-toml";
import { z } from "zod";
import { userInfo } from "node:os";
import type { Machine } from "../declaration.ts";
import { ObservationOpen, type Receipt } from "../receipt.ts";
import type { Remote } from "./remote.ts";
import { shellQuote } from "./owned-process.ts";
import { sampleConfigAccess } from "./config-access.ts";

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
export class CodexHomeGuard {
  private readonly machine: Machine;
  private readonly remote: Remote | undefined;
  private readonly before: GuardSnapshot;
  private hostWindow: GuardSnapshot | undefined;
  private requiredFilesReadable = true;
  private ownedWriteAccessDuringChange = false;
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
  static async capture(machine: Machine, remote?: Remote): Promise<CodexHomeGuard> {
    const normalHome =
      remote === undefined
        ? `${userInfo().homedir}/.codex`
        : (await remote.shell("printf '%s/.codex' \"$HOME\"")).trim();
    if (machine.codexHome !== normalHome) throw new Error("normal Codex home required");
    const guard = new CodexHomeGuard(machine, remote, { hashes: [], modifiedAt: [] });
    const cwd =
      remote === undefined
        ? await realpath(machine.nativeCwd)
        : (await remote.shell(`cd ${shellQuote(machine.nativeCwd)} && /bin/pwd -P`)).trim();
    if (cwd !== machine.nativeCwd || (remote === undefined && !(await stat(cwd)).isDirectory()))
      throw new Error("native cwd must be an existing canonical directory");
    const before = await guard.snapshot();
    const config = z
      .record(z.string(), z.unknown())
      .parse(parse(await guard.contents("config.toml")));
    const projects = z.record(z.string(), z.unknown()).parse(config.projects ?? {});
    if (!z.object({ trust_level: z.literal("trusted") }).safeParse(projects[cwd]).success)
      throw new Error(
        "native cwd lacks an existing exact trust entry; configuration left untouched",
      );
    const ready = new CodexHomeGuard(machine, remote, before);
    await ready.verify();
    return ready;
  }
  private async verifySnapshot(
    after: GuardSnapshot,
    receipt?: Receipt,
    ownedPids: readonly number[] = [],
  ): Promise<void> {
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
