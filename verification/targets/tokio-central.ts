import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import { decode, encode } from "../exact-json.ts";
import { OwnedProcess, privateDirectory, qualifyArtifact } from "../machines/owned-process.ts";
import { CentralOwner } from "../clients/central-owner.ts";
import type { Target } from "../declaration.ts";
import type { Receipt } from "../receipt.ts";
import type { CentralTarget } from "./central-target.ts";

export class TokioCentral implements CentralTarget {
  readonly origin: string;
  readonly owner: CentralOwner;
  readonly process: OwnedProcess;
  private constructor(origin: string, owner: CentralOwner, process: OwnedProcess) {
    this.origin = origin;
    this.owner = owner;
    this.process = process;
  }
  static async start(input: {
    target: Extract<Target, { kind: "tokio" }>;
    repository: string;
    receipt: Receipt;
  }): Promise<TokioCentral> {
    const { target, receipt } = input;
    await qualifyArtifact(target.binary);
    await mkdir(target.root, { mode: 0o700 });
    await privateDirectory(target.root);
    const request = `${receipt.directory}/tokio-request.json`;
    await writeFile(
      request,
      encode({ root: target.root, assertionFile: target.assertionFile, projects: target.projects }),
      { mode: 0o600, flag: "wx" },
    );
    const process = new OwnedProcess({
      executable: target.binary.path,
      arguments: [request],
      environment: { ...globalThis.process.env, TMPDIR: dirname(target.root) },
      cwd: input.repository,
      name: "tokio-central",
      receipt,
    });
    try {
      const ready = z
        .object({ kind: z.literal("ready"), pid: z.bigint(), url: z.url() })
        .parse(decode(await process.line()));
      const url = new URL(ready.url);
      if (
        ready.pid !== BigInt(process.pid) ||
        url.hostname !== "127.0.0.1" ||
        [8787, 8788, 43127].includes(Number(url.port))
      )
        throw new Error("Tokio readiness does not name owned loopback process");
      const assertion = (await readFile(target.assertionFile, "utf8")).trim();
      process.capture();
      await receipt.record("tokioReady", {
        origin: ready.url,
        pid: process.pid,
        binarySHA256: target.binary.sha256,
      });
      return new TokioCentral(
        ready.url,
        new CentralOwner(ready.url, { "Cf-Access-Jwt-Assertion": assertion }),
        process,
      );
    } catch (error: unknown) {
      await process.stop();
      throw error;
    }
  }
  async inspect(output: string): Promise<unknown> {
    this.process.child.stdin?.write(`${encode({ kind: "inspect", output })}\n`);
    return decode(await this.process.line());
  }
  async close(): Promise<void> {
    this.process.child.stdin?.end();
    const result = await Promise.race([
      this.process.done,
      setTimeout(20_000, "timeout", { ref: false }),
    ]);
    if (result === "timeout") {
      await this.process.stop();
      throw new Error("Tokio EOF shutdown timed out");
    }
    await this.process.stop();
  }
}
