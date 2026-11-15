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
import { randomUUIDv7 } from "node:crypto";

export type PreInferenceRefusal = "model_not_found" | "auth" | "selection";

export interface BudgetEvent {
  readonly kind: "reserved" | "refused" | "settled";
  readonly model: string;
  readonly sonnetUsed: number;
  readonly modelRequests: number;
  readonly sonnetBillable: number;
  readonly sonnetPending: number;
  readonly preInferenceRefusal: PreInferenceRefusal | null;
}

export class SonnetRequestBudget {
  private sonnetUsed: number;
  private readonly counts = new Map<string, number>();
  private readonly pending = new Map<string, string>();
  private readonly refused = new Map<string, number>();
  private blocked = 0;
  private readonly record: (event: BudgetEvent) => void;

  constructor(used: number, record: (event: BudgetEvent) => void) {
    if (!Number.isSafeInteger(used) || used < 0 || used > 3)
      throw new Error("invalid Sonnet budget");
    this.sonnetUsed = used;
    this.record = record;
  }

  reserve(model: string): string | null {
    const sonnet = model.toLowerCase().includes("sonnet");
    const inFlight = this.sonnetPending();
    if (sonnet && this.sonnetUsed + inFlight >= 3) {
      this.record(this.event("refused", model, this.sonnetUsed, inFlight, null));
      this.blocked++;
      return null;
    }
    const modelRequests = (this.counts.get(model) ?? 0) + 1;
    this.record({
      ...this.event("reserved", model, this.sonnetUsed, inFlight + (sonnet ? 1 : 0), null),
      modelRequests,
    });
    const id = randomUUIDv7();
    this.pending.set(id, model);
    this.counts.set(model, modelRequests);
    return id;
  }

  settle(id: string, refusal: PreInferenceRefusal | null): void {
    const model = this.pending.get(id);
    if (model === undefined) throw new Error("unknown or settled budget reservation");
    const sonnet = model.toLowerCase().includes("sonnet");
    const billable = this.sonnetUsed + (sonnet && refusal === null ? 1 : 0);
    const pending = this.sonnetPending() - (sonnet ? 1 : 0);
    this.record(this.event("settled", model, billable, pending, refusal));
    this.sonnetUsed = billable;
    this.pending.delete(id);
    if (refusal !== null) this.refused.set(model, (this.refused.get(model) ?? 0) + 1);
  }

  private sonnetPending(): number {
    return [...this.pending.values()].filter((model) => model.toLowerCase().includes("sonnet"))
      .length;
  }

  private event(
    kind: BudgetEvent["kind"],
    model: string,
    billable: number,
    pending: number,
    refusal: PreInferenceRefusal | null,
  ): BudgetEvent {
    return {
      kind,
      model,
      sonnetUsed: billable + pending,
      sonnetBillable: billable,
      sonnetPending: pending,
      modelRequests: this.counts.get(model) ?? 0,
      preInferenceRefusal: refusal,
    };
  }

  summary(): {
    sonnetUsed: number;
    sonnetBillable: number;
    sonnetPending: number;
    byModel: Record<string, number>;
    refusedByModel: Record<string, number>;
    blocked: number;
  } {
    return {
      sonnetUsed: this.sonnetUsed + this.sonnetPending(),
      sonnetBillable: this.sonnetUsed,
      sonnetPending: this.sonnetPending(),
      byModel: Object.fromEntries(this.counts),
      refusedByModel: Object.fromEntries(this.refused),
      blocked: this.blocked,
    };
  }
}

/** Only explicit HTTP error envelopes release a slot. Stream errors, transport
 * failures and ambiguous refusals remain conservatively charged. */
export function preInferenceRefusal(status: number, body: Buffer): PreInferenceRefusal | null {
  if (status < 400) return null;
  let value: unknown;
  try {
    value = JSON.parse(body.toString("utf8"));
  } catch {
    return null;
  }
  const parsed = z
    .object({ error: z.object({ type: z.string().optional(), code: z.string().optional() }) })
    .safeParse(value);
  if (!parsed.success) return null;
  const tags = [parsed.data.error.type, parsed.data.error.code];
  if (tags.includes("model_not_found")) return "model_not_found";
  if (
    (status === 401 || status === 403) &&
    tags.some((tag) => tag === "authentication_error" || tag === "invalid_api_key")
  )
    return "auth";
  if (
    tags.some(
      (tag) =>
        tag === "selection_rejected" ||
        tag === "no_eligible_accounts" ||
        tag === "no_accounts_available",
    )
  )
    return "selection";
  return null;
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
  budget: SonnetRequestBudget,
  reservation: string,
): Promise<void> {
  return new Promise((resolve, reject): void => {
    let settled = false;
    const settle = (refusal: PreInferenceRefusal | null): void => {
      if (!settled) {
        budget.settle(reservation, refusal);
        settled = true;
      }
    };
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
        void relay(returned, response, settle).then(resolve, reject);
      },
    );
    outgoing.on("error", (error): void => {
      settle(null);
      reject(error);
    });
    response.on("close", (): void => {
      if (!response.writableFinished) outgoing.destroy();
    });
    outgoing.end(body);
  });
}

async function relay(
  returned: IncomingMessage,
  response: ServerResponse,
  settle: (refusal: PreInferenceRefusal | null) => void,
): Promise<void> {
  const status = returned.statusCode ?? 502;
  if (status < 400) {
    settle(null);
    response.writeHead(status, returned.headers);
    await pipeline(returned, response);
    return;
  }
  const chunks: Buffer[] = [];
  let size = 0;
  let streaming = false;
  try {
    for await (const chunk of returned) {
      const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
      if (streaming) {
        if (!response.write(bytes)) await once(response, "drain");
        continue;
      }
      size += bytes.length;
      chunks.push(bytes);
      // An oversized error is ambiguous; never refund or log its contents.
      if (size > 65_536) {
        settle(null);
        response.writeHead(status, returned.headers);
        for (const buffered of chunks) response.write(buffered);
        chunks.length = 0;
        streaming = true;
      }
    }
    if (streaming) {
      response.end();
      return;
    }
    const body = Buffer.concat(chunks);
    settle(preInferenceRefusal(status, body));
    response.writeHead(status, returned.headers);
    response.end(body);
  } catch (error) {
    settle(null);
    throw error;
  }
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
    const reservation = input.budget.reserve(model);
    if (reservation === null) {
      response.writeHead(429, { "content-type": "application/json" });
      response.end('{"error":{"type":"budget_exhausted","message":"Sonnet request cap reached"}}');
      return;
    }
    await forward(
      new URL(`http://127.0.0.1:${input.routerPort}${path.pathname}${path.search}`),
      request,
      response,
      body,
      input.budget,
      reservation,
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
