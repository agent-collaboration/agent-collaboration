import {
  createServer,
  request,
  type Server,
  type IncomingMessage,
  type ServerResponse,
} from "node:http";
import type { Socket } from "node:net";

export class FixtureEdge {
  readonly origin: string;
  readonly server: Server;
  private readonly sockets = new Set<Socket>();
  private paused = false;
  private constructor(origin: string, server: Server) {
    this.origin = origin;
    this.server = server;
  }
  static async start(input: {
    upstream: string;
    assertion: string;
    owner: { readonly token: string; readonly assertion: string };
    accessId: string;
    accessSecret: string;
  }): Promise<FixtureEdge> {
    const upstream = new URL(input.upstream);
    if (upstream.protocol !== "http:" || upstream.hostname !== "127.0.0.1")
      throw new Error("fixture edge requires loopback central");
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
      throw new Error("fixture edge bind invalid");
    }
    const edge = new FixtureEdge(`http://127.0.0.1:${address.port}`, server);
    server.on("request", (incoming, response): void => {
      edge.forwardEnrollment({ incoming, response, upstream, owner: input.owner });
    });
    server.on("connection", (socket): void => {
      edge.sockets.add(socket);
      socket.once("close", (): void => {
        edge.sockets.delete(socket);
      });
    });
    server.on("upgrade", (incoming, downstream, head): void => {
      if (edge.paused) {
        downstream.destroy();
        return;
      }
      if (
        incoming.url !== "/v1/session" ||
        incoming.headers["cf-access-client-id"] !== input.accessId ||
        incoming.headers["cf-access-client-secret"] !== input.accessSecret
      ) {
        downstream.end(
          "HTTP/1.1 401 Unauthorized\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        );
        return;
      }
      const outgoing = request(new URL("/v1/session", upstream), {
        method: "GET",
        headers: {
          ...incoming.headers,
          host: upstream.host,
          "cf-access-jwt-assertion": input.assertion,
        },
      });
      outgoing.once("upgrade", (response, upstreamSocket, upstreamHead): void => {
        edge.sockets.add(upstreamSocket);
        upstreamSocket.once("close", (): void => {
          edge.sockets.delete(upstreamSocket);
          downstream.destroy();
        });
        const headers = response.rawHeaders.reduce(
          (text, value, index): string =>
            index % 2 === 0 ? `${text}${value}: ` : `${text}${value}\r\n`,
          "",
        );
        downstream.write(
          `HTTP/1.1 ${response.statusCode} ${response.statusMessage}\r\n${headers}\r\n`,
        );
        if (upstreamHead.length > 0) downstream.write(upstreamHead);
        if (head.length > 0) upstreamSocket.write(head);
        downstream.pipe(upstreamSocket);
        upstreamSocket.pipe(downstream);
        downstream.once("close", (): void => {
          upstreamSocket.destroy();
        });
      });
      outgoing.once("response", (response): void => {
        const headers = response.rawHeaders.reduce(
          (text, value, index): string =>
            index % 2 === 0 ? `${text}${value}: ` : `${text}${value}\r\n`,
          "",
        );
        downstream.write(
          `HTTP/1.1 ${response.statusCode} ${response.statusMessage}\r\n${headers}\r\n`,
        );
        response.pipe(downstream);
      });
      outgoing.once("error", (): void => {
        downstream.destroy();
      });
      outgoing.end();
    });
    return edge;
  }
  private forwardEnrollment(input: {
    incoming: IncomingMessage;
    response: ServerResponse;
    upstream: URL;
    owner: { readonly token: string; readonly assertion: string };
  }): void {
    const { incoming, response, upstream } = input;
    if (this.paused) {
      incoming.destroy();
      return;
    }
    if (incoming.method !== "POST" || incoming.url !== "/admin/v1/machines/enroll") {
      response.writeHead(404).end();
      return;
    }
    if (incoming.headers["cf-access-token"] !== input.owner.token) {
      response.writeHead(403).end();
      return;
    }
    const headers = { ...incoming.headers };
    delete headers["cf-access-token"];
    headers.host = upstream.host;
    headers["cf-access-jwt-assertion"] = input.owner.assertion;
    const outgoing = request(new URL("/admin/v1/machines/enroll", upstream), {
      method: "POST",
      headers,
      timeout: 30_000,
    });
    outgoing.once("response", (reply): void => {
      response.writeHead(reply.statusCode ?? 502, reply.rawHeaders);
      reply.pipe(response);
    });
    outgoing.once("timeout", (): void => {
      outgoing.destroy();
    });
    outgoing.once("error", (): void => {
      if (!response.headersSent) response.writeHead(502);
      response.end();
    });
    incoming.once("aborted", (): void => {
      outgoing.destroy();
    });
    response.once("close", (): void => {
      outgoing.destroy();
    });
    incoming.pipe(outgoing);
  }
  pause(): void {
    this.paused = true;
    for (const socket of this.sockets) socket.destroy();
  }
  resume(): void {
    this.paused = false;
  }
  async close(): Promise<void> {
    this.pause();
    await new Promise<void>((resolve, reject): void => {
      this.server.close((error): void => {
        if (error) reject(error);
        else resolve();
      });
    });
  }
}
