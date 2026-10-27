import assert from "node:assert/strict";
import { closeSync, existsSync, writeFileSync } from "node:fs";
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
    await input.next();
    if (scenario === "held") await setTimeout(60_000);
    else {
      assert.equal(scenario, "oversized");
      const limit = Number(process.argv[4]);
      assert.ok(Number.isSafeInteger(limit) && limit > 0);
      const frame = Buffer.alloc(limit + 2, 0x78);
      frame[limit + 1] = 0x0a;
      await new Promise<void>((resolve, reject): void => {
        process.stdout.write(frame, (error): void => {
          if (error) reject(error);
          else resolve();
        });
      });
      while (!(await input.next()).done) {
        /* Match the original stdin.read() after the complete flush. */
      }
    }
  } finally {
    lines.close();
    process.stdin.destroy();
  }
}
