import assert from "node:assert/strict";
import { mkdtemp, realpath, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { z } from "zod";
import { decode, encode } from "../exact-json.ts";
import { RouterMcp } from "../clients/router-mcp.ts";
import { Receipt } from "../receipt.ts";

const requestSchema = z.object({
  jsonrpc: z.literal("2.0"),
  id: z.string(),
  method: z.literal("tools/call"),
  params: z.object({ name: z.string(), arguments: z.record(z.string(), z.unknown()) }),
});
type FixtureRequest = z.infer<typeof requestSchema>;
export async function withMcpFixture(input: {
  reply(request: FixtureRequest): unknown;
  verify(client: RouterMcp, receipt: Receipt): Promise<void>;
}): Promise<void> {
  const root = await realpath(await mkdtemp(join(tmpdir(), "2026-10-mcp-contract-")));
  const errors: unknown[] = [];
  const server = createServer(async (request, response): Promise<void> => {
    try {
      let body = "";
      for await (const chunk of request) body += String(chunk);
      const wire = requestSchema.parse(decode(body));
      response.setHeader("content-type", "application/json");
      response.end(
        encode({ jsonrpc: "2.0", id: wire.id, result: { structuredContent: input.reply(wire) } }),
      );
    } catch (error: unknown) {
      errors.push(error);
      response.statusCode = 500;
      response.end("fixture rejected request");
    }
  });
  try {
    await new Promise<void>((resolve, reject): void => {
      server.once("error", reject);
      server.listen(0, "127.0.0.1", resolve);
    });
    const address = server.address();
    if (address === null || typeof address === "string") throw new Error("fixture address absent");
    await input.verify(new RouterMcp(`http://127.0.0.1:${address.port}/mcp`), new Receipt(root));
    assert.deepEqual(errors, []);
  } finally {
    server.closeAllConnections();
    await new Promise<void>((resolve, reject): void => {
      server.close((error): void => {
        if (error) reject(error);
        else resolve();
      });
    });
    await rm(root, { recursive: true });
  }
}
