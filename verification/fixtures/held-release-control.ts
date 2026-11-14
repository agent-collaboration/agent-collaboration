// Private control for the controlled ACP fixture, never a provider protocol.
import { chmodSync } from "node:fs";
import { createConnection, createServer, type Server } from "node:net";
import { once } from "node:events";
import { createInterface } from "node:readline";
import { z } from "zod";

const releaseRequest = z.object({ ownerId: z.string().min(1) }).strict();
const releaseReply = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("released"), ownerId: z.string() }).strict(),
  z.object({ kind: z.literal("refused"), ownerId: z.string() }).strict(),
]);

function parseReleaseLine(line: string): z.infer<typeof releaseRequest> | undefined {
  try {
    const parsed = releaseRequest.safeParse(JSON.parse(line));
    return parsed.success ? parsed.data : undefined;
  } catch {
    return undefined;
  }
}

export async function listenForHeldRelease(props: {
  socketPath: string;
  releaseOwner: (ownerId: string) => boolean;
  recordReply: (reply: z.infer<typeof releaseReply>) => void;
}): Promise<Server> {
  const server = createServer((connection): void => {
    const lines = createInterface({ input: connection });
    lines.once("line", (line: string): void => {
      const request = parseReleaseLine(line);
      const ownerId = request?.ownerId ?? "invalid";
      const kind = request !== undefined && props.releaseOwner(ownerId) ? "released" : "refused";
      const reply = releaseReply.parse({ kind, ownerId });
      props.recordReply(reply);
      connection.end(`${JSON.stringify(reply)}\n`);
      lines.close();
    });
    connection.on("error", (): void => {
      props.recordReply({ kind: "refused", ownerId: "disconnected" });
      lines.close();
      connection.destroy();
    });
    connection.once("close", (): void => lines.close());
  });
  server.listen(props.socketPath);
  await once(server, "listening");
  chmodSync(props.socketPath, 0o600);
  return server;
}

export async function signalHeldRelease(props: {
  socketPath: string;
  ownerId: string;
}): Promise<z.infer<typeof releaseReply>> {
  const connection = createConnection(props.socketPath);
  try {
    await once(connection, "connect");
    const lines = createInterface({ input: connection });
    connection.write(`${JSON.stringify(releaseRequest.parse({ ownerId: props.ownerId }))}\n`);
    try {
      const next = await lines[Symbol.asyncIterator]().next();
      if (next.done) throw new Error("release control ended without a reply");
      const reply = releaseReply.parse(JSON.parse(next.value));
      if (reply.ownerId !== props.ownerId) throw new Error("release reply changed owner");
      return reply;
    } finally {
      lines.close();
    }
  } finally {
    connection.destroy();
  }
}
