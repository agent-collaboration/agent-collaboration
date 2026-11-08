/** Qualify D4 window measurements against both original byte streams, never a sampled count. */
import { SessionAudit, type SessionSnapshot } from "../../../access/session-audit.ts";
import { SocketWindowObservation, type SocketWindowSnapshot } from "./window-observation.ts";

interface ConnectionAudit {
  readonly server: SessionAudit;
  readonly client: SessionAudit;
  readonly serverConnection: number;
  readonly clientConnection: number;
}
export interface WindowByteSnapshot {
  readonly window: SocketWindowSnapshot;
  readonly wire: readonly {
    readonly connection: string;
    readonly server: SessionSnapshot;
    readonly client: SessionSnapshot;
  }[];
}
export class WindowByteAudit {
  private readonly window = new SocketWindowObservation();
  private readonly connections = new Map<string, ConnectionAudit>();

  opened(connection: string): void {
    this.window.opened(connection);
    const server = new SessionAudit(
      (frame): void => this.window.server(connection, frame),
      "server",
      "batches",
    );
    const client = new SessionAudit(
      (frame): void => this.window.client(connection, frame),
      "client",
      "batches",
    );
    this.connections.set(connection, {
      server,
      client,
      serverConnection: server.opened(),
      clientConnection: client.opened(),
    });
  }
  server(connection: string, bytes: Buffer): void {
    const audit = this.connection(connection);
    audit.server.receive(audit.serverConnection, bytes);
  }
  client(connection: string, bytes: Buffer): void {
    const audit = this.connection(connection);
    audit.client.receive(audit.clientConnection, bytes);
  }
  bindPublicationSubject(connection: string, sendId: string, subject: string): void {
    this.window.bindPublicationSubject(connection, sendId, subject);
  }
  ended(connection: string): void {
    const audit = this.connection(connection);
    audit.server.ended(audit.serverConnection);
    audit.client.ended(audit.clientConnection);
    this.window.ended(connection);
  }
  private connection(connection: string): ConnectionAudit {
    const audit = this.connections.get(connection);
    if (audit === undefined) throw new Error("unobserved D4 physical socket");
    return audit;
  }
  snapshot(): WindowByteSnapshot {
    return {
      window: this.window.snapshot(),
      wire: [...this.connections].map(([connection, audit]) => ({
        connection,
        server: audit.server.snapshot(),
        client: audit.client.snapshot(),
      })),
    };
  }
  requireWindow(): void {
    for (const audit of this.connections.values()) {
      audit.server.requireComplete();
      audit.client.requireComplete();
      if (audit.server.snapshot().welcomes < 1 || (audit.client.snapshot().frames.hello ?? 0) < 1)
        throw new Error("D4 raw audit lacks actual Hello/Welcome per socket");
    }
    this.window.requireWindow();
  }
}
