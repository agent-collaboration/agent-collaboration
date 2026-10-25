import { z } from "zod";
import type { Artifact } from "../declaration.ts";
import { decode } from "../exact-json.ts";
import { command } from "../machines/owned-process.ts";
import type { Remote } from "../machines/remote.ts";
import { ObservationOpen } from "../receipt.ts";

export type CliOutcome =
  | { readonly kind: "answered"; readonly value: unknown }
  | { readonly kind: "refused"; readonly exit: number; readonly value: unknown }
  | { readonly kind: "unconfirmed"; readonly reason: "transport" | "invalidResponse" };
const envelopeSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("result"), result: z.unknown() }),
  z.object({ kind: z.literal("error"), error: z.unknown() }),
]);
export class RouterCli {
  readonly artifact: Artifact;
  readonly serviceDirectory: string;
  private readonly remote: Remote | undefined;
  constructor(artifact: Artifact, serviceDirectory: string, remote?: Remote) {
    this.artifact = artifact;
    this.serviceDirectory = serviceDirectory;
    this.remote = remote;
  }
  async outcome(arguments_: readonly string[]): Promise<CliOutcome> {
    const invocation = [...arguments_, "--service-directory", this.serviceDirectory, "--json"];
    let result;
    try {
      result =
        this.remote === undefined
          ? await command(this.artifact.path, invocation)
          : await this.remote.output([this.remote.pathFor(this.artifact), ...invocation]);
    } catch {
      return { kind: "unconfirmed", reason: "transport" };
    }
    if (result.exit === null || (this.remote !== undefined && result.exit === 255))
      return { kind: "unconfirmed", reason: "transport" };
    try {
      const envelope = envelopeSchema.parse(decode(result.stdout));
      if (envelope.kind === "result" && result.exit === 0 && envelope.result !== undefined)
        return { kind: "answered", value: envelope.result };
      if (envelope.kind === "error" && result.exit !== 0 && envelope.error !== undefined)
        return { kind: "refused", exit: result.exit, value: envelope.error };
      return { kind: "unconfirmed", reason: "invalidResponse" };
    } catch {
      return { kind: "unconfirmed", reason: "invalidResponse" };
    }
  }
  async call(arguments_: readonly string[]): Promise<unknown> {
    const result = await this.outcome(arguments_);
    if (result.kind === "answered") return result.value;
    if (result.kind === "unconfirmed")
      throw new ObservationOpen(`Router CLI ${result.reason}; effect unknown, no replay`);
    throw new Error("Router CLI refused; inspect outcome() for its typed result");
  }
}
