import assert from "node:assert/strict";
import { test } from "node:test";
import { listenOnTestPort } from "./loopback-test-port.ts";
import { PROTECTED_PORTS } from "../transport/streaming-mcp-client.ts";

test("a protected OS-assigned port is closed before binding another test port", async () => {
  const assigned = [43127, 50001];
  const events: string[] = [];
  let currentPort = 0;
  let listening = false;
  const server = {
    listen(port: number, host: string, ready: () => void): void {
      assert.equal(port, 0);
      assert.equal(host, "127.0.0.1");
      assert.equal(listening, false, "the protected listener must close before the next bind");
      const nextPort = assigned.shift();
      assert.notEqual(nextPort, undefined, "unexpected additional bind");
      if (nextPort === undefined) throw new Error("no scripted port");
      currentPort = nextPort;
      listening = true;
      events.push(`listen:${currentPort}`);
      ready();
    },
    address(): { readonly port: number } {
      return { port: currentPort };
    },
    close(closed: (error?: Error) => void): void {
      events.push(`close:${currentPort}`);
      listening = false;
      closed();
    },
  };
  assert.equal(PROTECTED_PORTS.has(43127), true);
  assert.equal(PROTECTED_PORTS.has(50001), false);
  const port = await listenOnTestPort(server);
  assert.equal(port, 50001);
  assert.equal(PROTECTED_PORTS.has(port), false);
  assert.deepEqual(events, ["listen:43127", "close:43127", "listen:50001"]);
  assert.equal(listening, true, "the accepted listener stays open for the test");
});
