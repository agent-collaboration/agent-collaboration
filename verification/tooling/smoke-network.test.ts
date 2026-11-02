import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { once } from "node:events";
import { createServer } from "node:net";
import { test } from "node:test";
import { z } from "zod";
import { freeLoopbackPort, listeningAddress, rejectionFrame } from "./smoke-network.ts";
import { observeRejectedWebsocket } from "./smoke-network-fixture.ts";

test("negative WebSocket probe retains original handshake, masked frame and completion", async (): Promise<void> => {
  const result = await observeRejectedWebsocket([
    process.execPath,
    new URL("./smoke-network.ts", import.meta.url).pathname,
    "websocket",
  ]);
  assert.deepEqual(
    result,
    JSON.parse(
      readFileSync(new URL("./fixtures/smoke-websocket-probe.json", import.meta.url), "utf8"),
    ),
  );
});
test("short masked frames match original bytes for fixed mask input", (): void => {
  const samples = z
    .array(z.object({ canary: z.string(), mask: z.string(), hex: z.string() }))
    .parse(
      JSON.parse(
        readFileSync(new URL("./fixtures/smoke-websocket-frames.json", import.meta.url), "utf8"),
      ),
    );
  for (const sample of samples)
    assert.equal(
      rejectionFrame(sample.canary, Buffer.from(sample.mask, "hex")).toString("hex"),
      sample.hex,
    );
});
test("listener readback matches original regex observations", (): void => {
  const samples = z
    .array(z.object({ text: z.string(), address: z.string().nullable() }))
    .parse(
      JSON.parse(readFileSync(new URL("./fixtures/smoke-listeners.json", import.meta.url), "utf8")),
    );
  for (const sample of samples) assert.equal(listeningAddress(sample.text) ?? null, sample.address);
});
test("free loopback port is released before returning it", async (): Promise<void> => {
  const port = await freeLoopbackPort();
  assert.ok(port >= 1 && port <= 65535);
  const server = createServer();
  try {
    server.listen(port, "127.0.0.1");
    await once(server, "listening");
  } finally {
    server.close();
  }
});
