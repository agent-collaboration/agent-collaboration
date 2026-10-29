import { createConnection, createServer, type Server, type Socket } from "node:net";
import { connect as connectTLS } from "node:tls";
import { setTimeout } from "node:timers/promises";
import { SessionAudit, type SessionSnapshot } from "./session-audit.ts";

export class SessionGate {
  readonly origin: string;
  readonly audit = new SessionAudit();
  private readonly server: Server;
  private readonly sockets = new Set<Socket>();
  private paused = false;
  private stopped = false;
  private constructor(origin: string, server: Server) {
    this.origin = origin;
    this.server = server;
  }
  static async start(endpoint: string): Promise<SessionGate> {
    const upstream = new URL(endpoint);
    if (
      !["http:", "https:", "ws:", "wss:"].includes(upstream.protocol) ||
      upstream.username !== "" ||
      upstream.password !== "" ||
      upstream.search !== "" ||
      upstream.hash !== "" ||
      upstream.pathname !== "/v1/session" ||
      [8787, 8788, 19741, 19742, 19743, 43127].includes(Number(upstream.port))
    )
      throw new Error("invalid declared Session upstream");
    const server = createServer();
    await new Promise<void>((resolve, reject): void => {
      server.once("error", reject);
      server.listen(0, "127.0.0.1", resolve);
    });
    const address = server.address();
    if (
      address === null ||
      typeof address === "string" ||
      [8787, 8788, 19741, 19742, 19743, 43127].includes(address.port)
    ) {
      server.close();
      throw new Error("invalid gate bind");
    }
    const gate = new SessionGate(`http://127.0.0.1:${address.port}`, server);
    server.on("connection", (socket): void => {
      gate.accept(socket, upstream);
    });
    return gate;
  }
  private track(socket: Socket): void {
    this.sockets.add(socket);
    socket.once("close", (): void => {
      this.sockets.delete(socket);
    });
    socket.on("error", (): void => {
      socket.destroy();
    });
  }
  private accept(downstream: Socket, upstream: URL): void {
    this.track(downstream);
    if (this.paused || this.stopped) {
      downstream.destroy();
      return;
    }
    downstream.setTimeout(10_000, (): void => {
      downstream.destroy();
    });
    let pending = Buffer.alloc(0);
    const handshake = (chunk: Buffer): void => {
      pending = Buffer.concat([pending, chunk]);
      const boundary = pending.indexOf("\r\n\r\n");
      if (boundary < 0) {
        if (pending.length > 65536) downstream.destroy();
        return;
      }
      downstream.off("data", handshake);
      downstream.pause();
      if (boundary > 65536) {
        downstream.destroy();
        return;
      }
      const lines = pending.subarray(0, boundary).toString("latin1").split("\r\n");
      const session = lines[0] === "GET /v1/session HTTP/1.1";
      if (!session && lines[0] !== "POST /admin/v1/machines/enroll HTTP/1.1") {
        downstream.end("HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
        return;
      }
      const forwarded = [
        lines[0],
        `Host: ${upstream.host}`,
        ...lines.slice(1).filter((line): boolean => !line.toLowerCase().startsWith("host:")),
      ];
      this.forward({
        downstream,
        upstream,
        session,
        initial: Buffer.concat([
          Buffer.from(`${forwarded.join("\r\n")}\r\n\r\n`, "latin1"),
          pending.subarray(boundary + 4),
        ]),
      });
      pending = Buffer.alloc(0);
    };
    downstream.on("data", handshake);
  }
  private forward(input: {
    downstream: Socket;
    upstream: URL;
    session: boolean;
    initial: Buffer;
  }): void {
    const { downstream, upstream } = input;
    const secure = ["https:", "wss:"].includes(upstream.protocol);
    const options = { host: upstream.hostname, port: Number(upstream.port) || (secure ? 443 : 80) };
    // TLS uses Node's trusted roots and upstream hostname verification.
    const outgoing = secure
      ? connectTLS({ ...options, servername: upstream.hostname })
      : createConnection(options);
    this.track(outgoing);
    const connection = input.session ? this.audit.opened() : undefined;
    outgoing.setTimeout(10_000, (): void => {
      outgoing.destroy();
    });
    downstream.once("close", (): void => {
      outgoing.destroy();
    });
    outgoing.once("close", (): void => {
      if (connection !== undefined) this.audit.ended(connection);
      downstream.destroy();
    });
    outgoing.once(secure ? "secureConnect" : "connect", (): void => {
      if (this.paused || this.stopped || downstream.destroyed) {
        outgoing.destroy();
        return;
      }
      downstream.setTimeout(0);
      outgoing.setTimeout(0);
      if (connection !== undefined)
        outgoing.on("data", (chunk: Buffer): void => {
          this.audit.receive(connection, chunk);
        });
      outgoing.write(input.initial);
      downstream.pipe(outgoing);
      outgoing.pipe(downstream);
      downstream.resume();
    });
  }
  pause(): void {
    this.paused = true;
    for (const socket of this.sockets) socket.destroy();
  }
  resume(): void {
    if (this.stopped) throw new Error("session gate is closed");
    this.paused = false;
  }
  async ready(previousWelcomes = 0): Promise<SessionSnapshot> {
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline) {
      if (this.stopped || !this.server.listening) throw new Error("session gate not listening");
      const snapshot = this.audit.snapshot();
      if (snapshot.invalidConnections > 0) throw new Error("Session observation invalid");
      if (snapshot.activeWelcomes > 0 && snapshot.welcomes > previousWelcomes) return snapshot;
      await setTimeout(100);
    }
    throw new Error("Session Welcome not observed through gate");
  }
  async close(): Promise<void> {
    this.stopped = true;
    this.pause();
    await new Promise<void>((resolve, reject): void => {
      this.server.close((error): void => (error === undefined ? resolve() : reject(error)));
    });
    if (this.sockets.size !== 0) throw new Error("gate sockets did not close");
  }
}
