import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { createServer } from "node:net";
import { withinFixtureDeadline } from "../../fixtures/fixture-control-socket.ts";

export async function observeRejectedWebsocket(command: readonly string[]): Promise<unknown> {
  let request: string | undefined;
  let frame: unknown;
  const server = createServer((socket): void => {
    let pending = Buffer.alloc(0);
    socket.on("data", (bytes: Buffer): void => {
      pending = Buffer.concat([pending, bytes]);
      if (request === undefined) {
        const end = pending.indexOf("\r\n\r\n");
        if (end === -1) return;
        request = pending
          .subarray(0, end + 4)
          .toString()
          .replace(/127\.0\.0\.1:\d+/, "127.0.0.1:<PORT>")
          .replace(/Sec-WebSocket-Key: [^\r]+/, "Sec-WebSocket-Key: <KEY>");
        pending = pending.subarray(end + 4);
        socket.write("HTTP/1.1 101 Switching Protocols\r\n\r\n");
      }
      if (pending.length < 6) return;
      const length = (pending[1] ?? 0) & 0x7f;
      if (pending.length < 6 + length) return;
      const mask = pending.subarray(2, 6);
      const payload = pending
        .subarray(6, 6 + length)
        .map((byte, index) => byte ^ (mask[index % 4] ?? 0));
      frame = {
        header: pending.subarray(0, 2).toString("hex"),
        maskBytes: mask.length,
        payload: payload.toString(),
        trailingBytes: pending.length - 6 - length,
      };
      socket.end("closed");
    });
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  assert.ok(address !== null && typeof address !== "string");
  const [executable, ...arguments_] = command;
  assert.ok(executable !== undefined);
  const child = spawn(executable, [...arguments_, `127.0.0.1:${address.port}`, "private-canary"], {
    stdio: ["ignore", "pipe", "pipe"],
  });
  const closed = once(child, "close");
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (bytes: Buffer): void => {
    stdout += bytes.toString();
  });
  child.stderr.on("data", (bytes: Buffer): void => {
    stderr += bytes.toString();
  });
  try {
    const [exit] = await withinFixtureDeadline(closed, 10000);
    return { exit, stdout, stderr, request, frame };
  } finally {
    if (child.exitCode === null && child.signalCode === null) {
      child.kill("SIGKILL");
      await closed;
    }
    server.close();
  }
}
