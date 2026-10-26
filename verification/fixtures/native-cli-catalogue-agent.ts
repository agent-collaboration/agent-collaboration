import assert from "node:assert/strict";
import { once } from "node:events";
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import { createConnection } from "node:net";
import { z } from "zod";

const pathSourceSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("path"), path: z.string() }),
  z.object({ kind: z.literal("environment"), name: z.string() }),
]);
const configurationSchema = z.discriminatedUnion("kind", [
  z.object({
    kind: z.literal("file"),
    output: pathSourceSchema,
    readLog: pathSourceSchema.optional(),
  }),
  z.object({ kind: z.literal("text"), text: z.string() }),
  z.object({ kind: z.literal("failure"), stderr: z.string(), exit: z.number().int() }),
  z.object({
    kind: z.literal("overflow"),
    stream: z.enum(["stdout", "stderr"]),
    bytes: z.number().int().nonnegative(),
  }),
  z.object({ kind: z.literal("socket"), socketPath: z.string(), diagnosticPath: z.string() }),
]);
function sourcePath(source: z.infer<typeof pathSourceSchema>): string {
  return source.kind === "path" ? source.path : z.string().parse(process.env[source.name]);
}
export async function runFixture(configuration: unknown): Promise<void> {
  const input = configurationSchema.parse(configuration);
  assert.deepEqual(process.argv.slice(2), ["agents", "--json", "--all"]);
  switch (input.kind) {
    case "file":
      if (input.readLog !== undefined) appendFileSync(sourcePath(input.readLog), "read\n");
      console.log(readFileSync(sourcePath(input.output), "utf8").replace(/\r\n?/g, "\n"));
      return;
    case "text":
      console.log(input.text);
      return;
    case "failure":
      process.stderr.write(input.stderr);
      process.exitCode = input.exit;
      return;
    case "overflow":
      if (input.stream === "stderr") {
        process.stderr.write("x".repeat(input.bytes));
        console.log("[]");
      } else console.log("x".repeat(input.bytes));
      return;
    case "socket": {
      const socket = createConnection(input.socketPath);
      try {
        await once(socket, "connect");
        socket.write(`${process.pid}\n`);
        await new Promise<void>((resolve, reject): void => {
          socket.once("data", (): void => {
            resolve();
          });
          socket.once("end", resolve);
          socket.once("error", reject);
        });
      } catch (error) {
        writeFileSync(
          input.diagnosticPath,
          error instanceof Error ? error.toString() : String(error),
        );
        throw error;
      } finally {
        socket.destroy();
      }
      return;
    }
  }
}
