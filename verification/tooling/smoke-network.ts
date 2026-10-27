import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { once } from "node:events";
import { readFileSync } from "node:fs";
import { createConnection, createServer, type Socket } from "node:net";
import { z } from "zod";
import { withinFixtureDeadline } from "../fixtures/fixture-control-socket.ts";

export async function freeLoopbackPort(): Promise<number> {
  const server = createServer();
  try {
    server.listen(0, "127.0.0.1");
    await once(server, "listening");
    const address = server.address();
    assert.ok(address !== null && typeof address !== "string");
    return address.port;
  } finally {
    await new Promise<void>((resolve, reject): void => {
      server.close((error): void => {
        if (error === undefined) resolve();
        else reject(error);
      });
    });
  }
}
export function listeningAddress(text: string): string | undefined {
  // Python's whitespace class also includes the four ASCII information separators.
  // oxlint-disable-next-line no-control-regex
  return /listening:[\p{White_Space}\x1c-\x1f]+(127\.0\.0\.1:\p{Decimal_Number}+)/u.exec(text)?.[1];
}
export function rejectionFrame(canary: string, mask: Buffer): Buffer {
  assert.equal(mask.length, 4);
  const payload = Buffer.from(`{"type":"response.create","input":"${canary}"}`);
  // Preserve the original short-frame encoding, including its input limit.
  const length = 0x80 | payload.length;
  assert.ok(length <= 255);
  const header = Buffer.from([0x81, length]);
  return Buffer.concat([
    header,
    mask,
    Buffer.from(payload.map((byte, index) => byte ^ (mask[index % 4] ?? 0))),
  ]);
}
async function receive(socket: Socket): Promise<void> {
  while (socket.readableLength === 0 && !socket.readableEnded && !socket.destroyed)
    await withinFixtureDeadline(once(socket, "readable"), 5000);
  if (socket.readableLength !== 0) socket.read(Math.min(4096, socket.readableLength));
}
export async function sendRejectedWebsocket(listener: string, canary: string): Promise<void> {
  const [host, portText, extra] = listener.split(":");
  assert.equal(extra, undefined);
  const port = z.coerce.number().int().parse(portText);
  const key = randomBytes(16).toString("base64");
  const frame = rejectionFrame(canary, randomBytes(4));
  const socket = createConnection({ host: z.string().parse(host), port });
  try {
    await withinFixtureDeadline(once(socket, "connect"), 5000);
    socket.write(
      `GET /v1/responses HTTP/1.1\r\nHost: ${host}:${port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: ${key}\r\nSec-WebSocket-Version: 13\r\n\r\n`,
    );
    await receive(socket);
    await new Promise<void>((resolve, reject): void => {
      socket.write(frame, (error): void => {
        if (error === null || error === undefined) resolve();
        else reject(error);
      });
    });
    try {
      await receive(socket);
    } catch (error) {
      if (!(error instanceof Error) || error.message !== "fixture control deadline") throw error;
    }
  } finally {
    socket.destroy();
  }
}
export async function main(): Promise<void> {
  const argument = (index: number): string => z.string().parse(process.argv[index]);
  switch (argument(2)) {
    case "free-port":
      console.log(await freeLoopbackPort());
      return;
    case "listener": {
      const address = listeningAddress(readFileSync(argument(3), "utf8"));
      if (address === undefined) process.exitCode = 1;
      else console.log(address);
      return;
    }
    case "websocket":
      await sendRejectedWebsocket(argument(3), argument(4));
      return;
    default:
      throw new Error("unknown smoke network operation");
  }
}
if (import.meta.main) await main();
