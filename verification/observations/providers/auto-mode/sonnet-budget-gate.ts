// Verification-only cost boundary: no fourth Sonnet inference reaches the
// allocated debug Router. Request bodies/headers are transient and never logged.
import {
  createServer,
  request as httpRequest,
  type IncomingMessage,
  type Server,
  type ServerResponse,
} from "node:http";
import { once } from "node:events";
import { pipeline } from "node:stream/promises";
import { z } from "zod";

export interface BudgetEvent {
  readonly kind: "reserved" | "refused";
  readonly model: string;
  readonly sonnetUsed: number;
  readonly modelRequests: number;
}

export class SonnetRequestBudget {
  private sonnetUsed: number;
  private readonly counts = new Map<string, number>();
  private readonly record: (event: BudgetEvent) => void;

  constructor(used: number, record: (event: BudgetEvent) => void) {
    if (!Number.isSafeInteger(used) || used < 0 || used > 3)
      throw new Error("invalid Sonnet budget");
    this.sonnetUsed = used;
    this.record = record;
  }

  reserve(model: string): boolean {
    const sonnet = model.toLowerCase().includes("sonnet");
    if (sonnet && this.sonnetUsed >= 3) {
      this.record({
        kind: "refused",
        model,
        sonnetUsed: this.sonnetUsed,
        modelRequests: this.counts.get(model) ?? 0,
      });
      return false;
    }
    // Persist the reservation before any HTTP connection, even if that call fails.
    const sonnetUsed = this.sonnetUsed + (sonnet ? 1 : 0);
    const modelRequests = (this.counts.get(model) ?? 0) + 1;
    this.record({ kind: "reserved", model, sonnetUsed, modelRequests });
    this.sonnetUsed = sonnetUsed;
    this.counts.set(model, modelRequests);
    return true;
  }

  summary(): { sonnetUsed: number; byModel: Record<string, number> } {
    return { sonnetUsed: this.sonnetUsed, byModel: Object.fromEntries(this.counts) };
  }
}

const modelSchema = z
  .string()
  .min(1)
  .max(200)
  .regex(/^[a-zA-Z0-9._:[\]-]+$/);
async function readModelBody(request: IncomingMessage): Promise<{ body: Buffer; model: string }> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of request) {
    const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
    size += bytes.length;
    if (size > 8 * 1024 * 1024) throw new Error("budget request too large");
    chunks.push(bytes);
  }
  const body = Buffer.concat(chunks);
  // Only the top-level model leaves this function's in-memory JSON parse.
  const model = z.object({ model: modelSchema }).parse(JSON.parse(body.toString("utf8"))).model;
  return { body, model };
}

function forward(
  upstream: URL,
  incoming: IncomingMessage,
  response: ServerResponse,
  body: Buffer,
): Promise<void> {
  return new Promise((resolve, reject): void => {
    const headers = {
      ...incoming.headers,
      host: upstream.host,
      "content-length": String(body.length),
    };
    delete headers["transfer-encoding"];
    const outgoing = httpRequest(
      upstream,
      { method: incoming.method, headers },
      (returned): void => {
        response.writeHead(returned.statusCode ?? 502, returned.headers);
        void pipeline(returned, response).then(resolve, reject);
      },
    );
    outgoing.on("error", reject);
    response.on("close", (): void => {
      if (!response.writableFinished) outgoing.destroy();
    });
    outgoing.end(body);
  });
}

export async function startSonnetBudgetGate(input: {
  readonly routerPort: number;
  readonly budget: SonnetRequestBudget;
}): Promise<{ readonly baseUrl: string; readonly close: () => Promise<void> }> {
  if (!Number.isSafeInteger(input.routerPort) || input.routerPort < 1 || input.routerPort > 65535)
    throw new Error("invalid allocated debug Router port");
  const server = createServer((request, response): void => {
    void handle(request, response).catch((): void => {
      if (!response.headersSent) response.writeHead(502, { "content-type": "application/json" });
      response.end(
        '{"error":{"type":"budget_gate_error","message":"Verification budget gate refused this request"}}',
      );
    });
  });
  async function handle(request: IncomingMessage, response: ServerResponse): Promise<void> {
    const path = new URL(request.url ?? "/", "http://127.0.0.1");
    if (request.method !== "POST" || path.pathname !== "/anthropic/v1/messages") {
      response.writeHead(403);
      response.end();
      return;
    }
    const { body, model } = await readModelBody(request);
    if (!input.budget.reserve(model)) {
      response.writeHead(429, { "content-type": "application/json" });
      response.end('{"error":{"type":"budget_exhausted","message":"Sonnet request cap reached"}}');
      return;
    }
    await forward(
      new URL(`http://127.0.0.1:${input.routerPort}${path.pathname}${path.search}`),
      request,
      response,
      body,
    );
  }
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  if (address === null || typeof address === "string") throw new Error("budget gate bind failed");
  return {
    baseUrl: `http://127.0.0.1:${address.port}/anthropic`,
    close: (): Promise<void> => closeServer(server),
  };
}

async function closeServer(server: Server): Promise<void> {
  server.closeAllConnections();
  await new Promise<void>((resolve, reject): void => {
    server.close((error) => (error ? reject(error) : resolve()));
  });
}
