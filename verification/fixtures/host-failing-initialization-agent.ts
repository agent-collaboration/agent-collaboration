import assert from "node:assert/strict";
import { once } from "node:events";
import { closeSync, existsSync, writeFileSync } from "node:fs";
import { createConnection } from "node:net";
import { createInterface } from "node:readline";
import { setTimeout } from "node:timers/promises";

const scenario = process.argv[2];
const processIdPath = process.argv[3];
assert.ok(processIdPath);
writeFileSync(processIdPath, String(process.pid));
if (scenario === "eof") {
  const { AcpExchange } = await import("./acp-exchange.ts");
  const agent = new AcpExchange();
  agent.respond(await agent.read(), {
    protocolVersion: 1,
    agentCapabilities: {},
    agentInfo: { name: "eof-fixture", version: "1" },
  });
  const closeMarker = process.argv[4];
  assert.ok(closeMarker);
  while (!existsSync(closeMarker)) await setTimeout(10);
  closeSync(1);
  await agent.drain();
  agent.close();
} else {
  // Raw failure frames need no JSON setup. Write bytes directly so fixture
  // startup and string encoding do not consume the initialization deadline.
  const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
  const input = lines[Symbol.asyncIterator]();
  try {
    assert.ok(!(await input.next()).done, "expected initialize before EOF");
    const socketOrLimit = process.argv[4];
    assert.ok(socketOrLimit);
    // The packed exchange supplies the frame size and a flush witness path.
    // Host tests supply an initialize-observed socket before the frame size.
    const packedOversized = scenario === "oversized" && /^\d+$/.test(socketOrLimit);
    if (!packedOversized) {
      const observed = createConnection(socketOrLimit);
      await once(observed, "connect");
      observed.end();
    }
    if (scenario !== "held") {
      assert.equal(scenario, "oversized");
      const limit = Number(packedOversized ? socketOrLimit : process.argv[5]);
      assert.ok(Number.isSafeInteger(limit) && limit > 0);
      const frame = Buffer.alloc(limit + 2, 0x78);
      frame[limit + 1] = 0x0a;
      await new Promise<void>((resolve, reject): void => {
        process.stdout.write(frame, (error): void => {
          if (error) reject(error);
          else resolve();
        });
      });
      if (packedOversized) {
        const frameWrittenPath = process.argv[5];
        assert.ok(frameWrittenPath);
        writeFileSync(frameWrittenPath, String(process.pid));
      }
    }
    while (!(await input.next()).done) {
      /* Keep stdin open while the Router owns the failed initialization. */
    }
  } finally {
    lines.close();
    process.stdin.destroy();
  }
}
