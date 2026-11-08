import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createConnection, Socket } from "node:net";
import { once } from "node:events";
import { FixtureEdge } from "./fixture-edge.ts";

for (const resetFrom of ["upstream", "downstream"]) {
  test(
    `fixture Access edge settles a real upgraded-socket reset from ${resetFrom}`,
    { timeout: 5000 },
    async (): Promise<void> => {
      const upstream = createServer();
      const sockets = new Set<Socket>();
      upstream.on("connection", (socket): void => {
        sockets.add(socket);
        socket.on("error", (): void => {});
        socket.on("close", (): void => {
          sockets.delete(socket);
        });
      });
      let upgraded: Socket | undefined;
      upstream.on("upgrade", (_request, socket): void => {
        // Node's server upgrade event supplies the underlying net.Socket.
        if (!(socket instanceof Socket)) throw new Error("TCP socket required");
        upgraded = socket;
        socket.write(
          "HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n",
        );
      });
      await new Promise<void>((resolve): void => {
        upstream.listen(0, "127.0.0.1", resolve);
      });
      const address = upstream.address();
      assert.ok(address !== null && typeof address !== "string");
      const edge = await FixtureEdge.start({
        upstream: `http://127.0.0.1:${address.port}`,
        assertion: "synthetic-assertion",
        owner: { token: "synthetic-owner", assertion: "synthetic-owner-assertion" },
        accessId: "synthetic-client",
        accessSecret: "synthetic-secret",
      });
      const client = createConnection({
        host: "127.0.0.1",
        port: Number(new URL(edge.origin).port),
      });
      client.on("error", (): void => {});
      try {
        await once(client, "connect");
        const ready = once(client, "data");
        client.write(
          "GET /v1/session HTTP/1.1\r\nHost: fixture\r\nConnection: Upgrade\r\nUpgrade: websocket\r\ncf-access-client-id: synthetic-client\r\ncf-access-client-secret: synthetic-secret\r\n\r\n",
        );
        const [response] = await ready;
        assert.match(String(response), /^HTTP\/1\.1 101 /);
        assert.ok(upgraded !== undefined);
        const closed = new Promise<void>((resolve): void => {
          client.once("close", (): void => resolve());
        });
        if (resetFrom === "upstream") upgraded.resetAndDestroy();
        else client.resetAndDestroy();
        await closed;
        // No process-level uncaught socket error may bypass the controller's cleanup.
      } finally {
        client.destroy();
        await edge.close();
        for (const socket of sockets) socket.destroy();
        await new Promise<void>((resolve, reject): void => {
          upstream.close((error): void => (error ? reject(error) : resolve()));
        });
      }
    },
  );
}
