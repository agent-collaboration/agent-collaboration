import { createConnection } from "node:net";
import { z } from "zod";
import { OwnedProcess, shellQuote } from "./owned-process.ts";
import { Remote } from "./remote.ts";
import type { Receipt } from "../receipt.ts";
import { setTimeout } from "node:timers/promises";

export class Link {
  readonly remote: Remote;
  readonly port: number;
  readonly process: OwnedProcess;
  private constructor(remote: Remote, port: number, process: OwnedProcess) {
    this.remote = remote;
    this.port = port;
    this.process = process;
  }
  static async start(input: {
    remote: Remote;
    port: number;
    receipt: Receipt;
    repository: string;
  }): Promise<Link> {
    if ([8787, 8788, 43127].includes(input.port)) throw new Error("protected tunnel port");
    if ((await input.remote.listenerAddresses(input.port)).length !== 0)
      throw new Error("remote reverse-forward port collision");
    const process = new OwnedProcess({
      executable: "/usr/bin/ssh",
      arguments: [
        "-N",
        "-T",
        "-o",
        "ControlMaster=no",
        "-o",
        "ControlPath=none",
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "ServerAliveInterval=5",
        "-o",
        "ServerAliveCountMax=2",
        "-R",
        `127.0.0.1:${input.port}:127.0.0.1:${input.port}`,
        input.remote.machine.host,
      ],
      environment: globalThis.process.env,
      cwd: input.repository,
      name: "reverse-link",
      receipt: input.receipt,
    });
    const link = new Link(input.remote, input.port, process);
    try {
      let failure: unknown;
      for (let attempt = 0; attempt < 20; attempt += 1) {
        try {
          await link.assertAlive();
          return link;
        } catch (error: unknown) {
          failure = error;
        }
        if (process.child.exitCode !== null || process.child.signalCode !== null) break;
        await setTimeout(250);
      }
      throw failure;
    } catch (error: unknown) {
      await process.stop();
      throw error;
    }
  }
  async assertAlive(): Promise<void> {
    if (this.process.child.exitCode !== null || this.process.child.signalCode !== null)
      throw new Error("SSH link dropped; affected case invalid");
    const addresses = await this.remote.listenerAddresses(this.port);
    await this.process.receipt.record("reverseLinkBinding", {
      port: this.port,
      addresses,
      source: "netstat network socket table",
    });
    if (
      addresses.length === 0 ||
      addresses.some((address): boolean => address !== `127.0.0.1.${this.port}`)
    )
      throw new Error("reverse link is not exclusively remote loopback");
    await new Promise<void>((resolve, reject): void => {
      const socket = createConnection({ host: "127.0.0.1", port: this.port, timeout: 3000 });
      socket.once("connect", (): void => {
        socket.destroy();
        resolve();
      });
      socket.once("timeout", (): void => {
        socket.destroy();
        reject(new Error("local edge reachability timed out"));
      });
      socket.once("error", reject);
    });
    const yes = shellQuote(JSON.stringify({ reachable: true }));
    const no = shellQuote(JSON.stringify({ reachable: false }));
    z.object({ reachable: z.literal(true) }).parse(
      await this.remote.json(
        `if /usr/bin/nc -z -w 3 127.0.0.1 ${this.port}; then printf '%s' ${yes}; else printf '%s' ${no}; fi`,
      ),
    );
  }
  async close(): Promise<void> {
    await this.process.stop();
    for (let attempt = 0; attempt < 10; attempt += 1) {
      if ((await this.remote.listenerAddresses(this.port)).length === 0) return;
      await setTimeout(200);
    }
    throw new Error("owned reverse-forward port was not released");
  }
}
