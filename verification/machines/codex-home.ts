import { createHash } from "node:crypto";
import { readFile, realpath, stat } from "node:fs/promises";
import { parse } from "smol-toml";
import { z } from "zod";
import { userInfo } from "node:os";
import type { Machine } from "../declaration.ts";
import type { Receipt } from "../receipt.ts";
import type { Remote } from "./remote.ts";
import { shellQuote } from "./owned-process.ts";

const files = ["config.toml", "codex-router-debug.config.toml"];
export class CodexHomeGuard {
  private readonly machine: Machine;
  private readonly remote: Remote | undefined;
  private readonly before: readonly string[];
  private constructor(machine: Machine, remote: Remote | undefined, before: readonly string[]) {
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
  private async snapshot(): Promise<readonly string[]> {
    return Promise.all(
      files.map(async (name): Promise<string> =>
        createHash("sha256")
          .update(await this.contents(name))
          .digest("hex"),
      ),
    );
  }
  static async capture(machine: Machine, remote?: Remote): Promise<CodexHomeGuard> {
    const normalHome =
      remote === undefined
        ? `${userInfo().homedir}/.codex`
        : (await remote.shell("printf '%s/.codex' \"$HOME\"")).trim();
    if (machine.codexHome !== normalHome) throw new Error("normal Codex home required");
    const guard = new CodexHomeGuard(machine, remote, []);
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
  async verify(receipt?: Receipt): Promise<void> {
    const after = await this.snapshot();
    const unchanged =
      this.before.length === after.length &&
      this.before.every((hash, index): boolean => hash === after[index]);
    if (receipt !== undefined)
      await receipt.record("codexHomeGuard", {
        machine: this.machine.name,
        unchanged,
        before: this.before,
        after,
      });
    if (!unchanged)
      throw new Error("Codex configuration changed; observation stopped, files left untouched");
  }
}
