// Test support: a scripted Router stand-in for the driver's own case tests. It answers ordinary
// MCP calls in the shapes the real Host recorded on the first controlled runs (mcf11 and mcf13
// exchanges: admissions, completed results, terminal snapshots, operation waits, bounded
// observations and typed refusals), backed by a real temporary owning store and peer receipt.
// It decides nothing and stands in only for what a Host would answer; it is never Host,
// runtime or provider proof.
import { appendFileSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { tmpdir, userInfo } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { z } from "zod";
import { decode, encode } from "../../../exact-json.ts";
import type { RecordedExchange } from "../observations/history-replay-oracle.ts";

export const STAND_IN_SERVICE = "11111111-1111-4111-8111-111111111111";
export const STAND_IN_EPOCH = "22222222-2222-4222-8222-222222222222";
export const STAND_IN_OWNER = userInfo().username;
export const STAND_IN_REPLY = "beforeafter";

type Json = Readonly<Record<string, unknown>>;
export interface StandInTarget {
  readonly endpoint: { readonly serviceId: string; readonly endpointId: string };
  readonly sessionId: string;
}
export type { RecordedExchange } from "../observations/history-replay-oracle.ts";

/** How a reattach publishes history in its new epoch; "faithful" is what the real Host does. */
export type ReplayShape =
  | "none"
  | "faithful"
  | "canned"
  | "reordered"
  | "missing"
  | "liveLooking"
  | "foreign";

export interface StandInOptions {
  readonly endpointId?: string;
  /** What resume publishes; the real Host publishes no history after resume. */
  readonly resumeReplay?: ReplayShape;
  /** What load publishes; the real Host replays the recorded conversation. */
  readonly loadReplay?: ReplayShape;
  /** The Resumed settlement's history field; the real Host reports historyUnavailable. */
  readonly resumeHistory?: string;
  /** Overrides only the resume settlement kind. */
  readonly resumeSettlementKind?: string;
  /** Overrides only the load settlement kind, leaving resume valid. */
  readonly loadSettlementKind?: string;
  /** Every prompt gets the same reply, as a real provider may give; only the inputs differ. */
  readonly repeatedReply?: boolean;
  /** Observation reads return at most this many events. */
  readonly readChunk?: number;
  /** Every read reports resultLimitReached, so no history baseline can complete. */
  readonly alwaysLimited?: boolean;
  /** Tools refused before dispatch as unsupportedCapability, as a provider without them is. */
  readonly unsupported?: readonly string[];
  /** Also writes a store record for a refused operation, which a real refusal never has. */
  readonly recordRefused?: boolean;
  /** provider_session_inspect state; the real reopened Host reports unloaded before load. */
  readonly sessionState?: string;
  /** provider_session_inspect names this Session instead of the one asked about. */
  readonly inspectSessionId?: string;
  /** The serviceEpoch every binding snapshot names. */
  readonly bindingEpoch?: string;
  /** The live endpoint inventory (endpoints_list): its epoch, availability and provider channel. */
  readonly inventory?: {
    readonly epoch?: string;
    readonly unavailable?: boolean;
    readonly withoutProviderChannel?: boolean;
  };
  /** The binding the load operation's snapshot names, when it differs from the live one. */
  readonly loadBinding?: { readonly epoch?: string; readonly bindingId?: string };
  /** Which reattach loses its retained settlement, as after Host restart or eviction. */
  readonly unretainedSettlement?: "resumed" | "loaded";
  /** The prompt after a load: a live turn, no new turn (a stale terminal) or one with an injected input. */
  readonly afterLoadPrompt?: "live" | "stale" | "injected";
  /** Sessions that already exist, as after a settled first Host lifetime. */
  readonly existing?: readonly {
    readonly sessionId: string;
    readonly history: RecordedExchange[];
    /** The stored creator, when it no longer matches what the first lifetime recorded. */
    readonly createdBy?: string;
  }[];
}

/** The live binding the stand-in's endpoint inventory and snapshots name. */
export const STAND_IN_BINDING = "33333333-0000-4000-8000-000000000001";

interface SessionState {
  epoch: bigint;
  next: bigint;
  events: { sequence: bigint; event: Json }[];
  history: RecordedExchange[];
  loaded: boolean;
}

export interface StandIn {
  readonly url: string;
  readonly store: string;
  readonly receipt: string;
  readonly root: string;
  readonly calls: string[];
  readonly sessions: Map<string, SessionState>;
  close(): Promise<void>;
}

const callSchema = z.object({
  id: z.union([z.string(), z.number(), z.bigint()]),
  params: z.object({ name: z.string(), arguments: z.record(z.string(), z.unknown()) }),
});

function target(options: StandInOptions, sessionId: string): StandInTarget {
  return {
    endpoint: { serviceId: STAND_IN_SERVICE, endpointId: options.endpointId ?? "cursor-local" },
    sessionId,
  };
}

function item(itemId: string, kind: string, text: string): Json {
  return { itemId, kind: { kind }, text };
}

/** One live or historical turn, in the producer's event order and outcome shape. */
function turnEvents(
  turnId: string,
  exchange: RecordedExchange,
  stopReason: unknown,
): readonly Json[] {
  return [
    { kind: "turnStarted", turnId, inputId: `${turnId}-input` },
    { kind: "itemStarted", item: item(`${turnId}-user`, "userMessage", exchange.prompt) },
    { kind: "itemCompleted", itemId: `${turnId}-user` },
    { kind: "itemUpdated", item: item(`${turnId}-agent`, "agentMessage", exchange.reply) },
    { kind: "itemCompleted", itemId: `${turnId}-agent` },
    {
      kind: "turnEnded",
      turnId,
      outcome: { kind: "ended", stop_reason: stopReason, local_cause: null },
    },
  ];
}

function replayed(history: readonly RecordedExchange[], shape: ReplayShape): RecordedExchange[] {
  switch (shape) {
    case "none":
      return [];
    case "canned":
      return history.map(() => ({ prompt: "canned prompt", reply: "canned reply" }));
    case "reordered":
      return [...history].reverse();
    case "missing":
      return history.slice(0, -1);
    case "foreign":
      return history.map((exchange) => ({
        ...exchange,
        reply: `${exchange.reply} (other Session)`,
      }));
    default:
      return [...history];
  }
}

const OPERATION_TABLES =
  "CREATE TABLE provider_session_records (target_service_id TEXT, target_endpoint_id TEXT, target_session_id TEXT, created_by_json TEXT, approver_json TEXT); CREATE TABLE provider_operations (operation_id TEXT, operation_kind TEXT, target_service_id TEXT, target_endpoint_id TEXT, target_session_id TEXT, stage TEXT, effect TEXT, reconciliation_state TEXT, admitted_at_ms INTEGER, terminal_at_ms INTEGER);";

type Answer = { readonly result: Json } | { readonly error: Json };

/** The scripted Host behind the stand-in server: one method per tool it answers. */
class ScriptedHost {
  readonly sessions = new Map<string, SessionState>();
  private readonly settlements = new Map<string, Json>();
  private readonly kinds = new Map<string, string>();
  private created = 0;
  private readonly options: StandInOptions;
  readonly store: string;
  readonly receipt: string;

  constructor(options: StandInOptions, store: string, receipt: string) {
    this.options = options;
    this.store = store;
    this.receipt = receipt;
    const database = new DatabaseSync(store);
    database.exec(OPERATION_TABLES);
    database.close();
    writeFileSync(receipt, "");
    for (const existing of options.existing ?? []) {
      const session: SessionState = {
        epoch: 0n,
        next: 0n,
        events: [],
        history: [...existing.history],
        loaded: false,
      };
      this.sessions.set(existing.sessionId, session);
      this.recordSession(existing.sessionId, existing.createdBy);
    }
  }

  answer(name: string, args: Json): Answer {
    const sessionId = z.object({ target: z.object({ sessionId: z.string() }) }).safeParse(args).data
      ?.target.sessionId;
    const operationId = typeof args["operationId"] === "string" ? args["operationId"] : "";
    switch (name) {
      case "conversation_create":
        return this.create(operationId);
      case "events_observe":
        return this.observe(this.session(name, sessionId), args);
      case "conversation_prompt":
        return this.prompt(this.session(name, sessionId), operationId, args);
      case "conversation_close":
      case "conversation_resume":
        return this.closeOrResume(this.session(name, sessionId), name, operationId);
      case "conversation_load":
        return this.load(this.session(name, sessionId), operationId);
      case "conversation_operation_show":
        return { result: this.snapshot(z.string().parse(args["operationId"])) };
      case "conversation_operation_wait":
        return this.wait(z.string().parse(args["operationId"]));
      case "provider_session_inspect":
        return {
          result: {
            target: this.target(this.options.inspectSessionId ?? this.session(name, sessionId).id),
            state: this.options.sessionState ?? "unloaded",
            capabilities: {},
            history: "available",
          },
        };
      case "endpoints_list":
        return { result: this.inventory() };
      default:
        throw new Error(`the stand-in has no answer for ${name}`);
    }
  }

  private session(tool: string, sessionId: string | undefined): SessionState & { id: string } {
    const session = sessionId === undefined ? undefined : this.sessions.get(sessionId);
    if (session === undefined || sessionId === undefined)
      throw new Error(`${tool}: unknown Session`);
    return Object.assign(session, { id: sessionId });
  }

  private target(sessionId: string): StandInTarget {
    return target(this.options, sessionId);
  }

  private create(operationId: string): Answer {
    this.created += 1;
    const id = `session-${this.created}`;
    const fresh: SessionState = { epoch: -1n, next: 0n, events: [], history: [], loaded: false };
    this.sessions.set(id, fresh);
    this.openEpoch(fresh, [], null);
    this.recordSession(id);
    this.recordOperation(operationId, "conversationCreate", id);
    return { result: { kind: "created", operationId, target: this.target(id) } };
  }

  private observe(session: SessionState & { id: string }, args: Json): Answer {
    const after = typeof args["afterSequence"] === "bigint" ? args["afterSequence"] : 0n;
    const pending = session.events.filter((event) => event.sequence > after);
    const events = pending.slice(0, this.options.readChunk ?? 200);
    const limited = this.options.alwaysLimited === true || pending.length > events.length;
    return {
      result: {
        target: this.target(session.id),
        generation: { serviceEpoch: this.options.bindingEpoch ?? STAND_IN_EPOCH, generation: 2 },
        attached: true,
        events,
        endReason: limited ? "resultLimitReached" : "deadlineReached",
        continuationGap: true,
        epoch: session.epoch,
      },
    };
  }

  private prompt(session: SessionState & { id: string }, operationId: string, args: Json): Answer {
    const message = z.object({ text: z.string() }).parse(args["message"]);
    // Each turn's reply is distinct, so a reordered replay is visible.
    const exchange = {
      prompt: message.text,
      reply:
        this.options.repeatedReply === true
          ? STAND_IN_REPLY
          : `${STAND_IN_REPLY} ${session.history.length + 1}`,
    };
    const shape = session.loaded ? (this.options.afterLoadPrompt ?? "live") : "live";
    if (shape !== "stale") {
      const events = [...turnEvents(`live-${operationId}`, exchange, "endTurn")];
      if (shape === "injected")
        events.splice(1, 0, {
          kind: "inputAccepted",
          inputId: "injected-input",
          turnId: `live-${operationId}`,
        });
      this.publish(session, events);
    }
    session.history.push(exchange);
    this.recordOperation(operationId, "conversationPrompt", session.id);
    this.receiptRow("session/prompt", session.id);
    return {
      result: {
        kind: "completed",
        target: this.target(session.id),
        operationId,
        settlement: {
          target: this.target(session.id),
          stopReason: "endTurn",
          detail: { kind: "providerPrompt", output: { kind: "available", text: exchange.reply } },
        },
      },
    };
  }

  private closeOrResume(
    session: SessionState & { id: string },
    name: string,
    operationId: string,
  ): Answer {
    const tool = name === "conversation_close" ? "close" : "resume";
    if ((this.options.unsupported ?? []).includes(tool))
      return this.refuse(operationId, session.id, tool);
    this.recordOperation(
      operationId,
      tool === "close" ? "conversationClose" : "conversationResume",
      session.id,
    );
    this.receiptRow(`session/${tool}`, session.id);
    if (tool === "resume") {
      this.openEpoch(session, replayed(session.history, this.options.resumeReplay ?? "none"), {
        unknown: "replayed",
      });
      this.settlements.set(operationId, {
        kind: this.options.resumeSettlementKind ?? "resumed",
        target: this.target(session.id),
        effectiveSettings: {},
        history: this.options.resumeHistory ?? "historyUnavailable",
      });
    }
    return {
      result: { admission: "admitted", operation: this.snapshot(operationId, "mayHaveDispatched") },
    };
  }

  private load(session: SessionState & { id: string }, operationId: string): Answer {
    if ((this.options.unsupported ?? []).includes("load"))
      return this.refuse(operationId, session.id, "load");
    this.recordOperation(operationId, "conversationLoad", session.id);
    this.receiptRow("session/load", session.id);
    const shape = this.options.loadReplay ?? "faithful";
    this.openEpoch(
      session,
      replayed(session.history, shape),
      shape === "liveLooking" ? "endTurn" : { unknown: "replayed" },
    );
    session.loaded = true;
    this.settlements.set(operationId, {
      kind: this.options.loadSettlementKind ?? "loaded",
      target: this.target(session.id),
      effectiveSettings: {},
    });
    return {
      result: {
        kind: "completed",
        target: this.target(session.id),
        operationId,
        settlement: {
          target: this.target(session.id),
          detail: { kind: "providerLoad", output: { kind: "available", effectiveSettings: {} } },
        },
      },
    };
  }

  private wait(operationId: string): Answer {
    const retained = this.settlements.get(operationId);
    const settlement =
      this.options.unretainedSettlement !== undefined &&
      retained?.kind === this.options.unretainedSettlement
        ? undefined
        : retained;
    return {
      result: {
        operation: this.snapshot(operationId),
        output:
          settlement === undefined
            ? { kind: "outputUnavailable", reason: "notRetained" }
            : { kind: "available", settlement },
      },
    };
  }

  /** endpoints_list in the recorded shape (endpoint_inventory.rs, endpoint_description.rs). */
  private inventory(): Json {
    const inventory = this.options.inventory ?? {};
    return {
      serviceEpoch: inventory.epoch ?? this.options.bindingEpoch ?? STAND_IN_EPOCH,
      sequence: 1,
      endpoints: [
        {
          endpoint: {
            serviceId: STAND_IN_SERVICE,
            endpointId: this.options.endpointId ?? "cursor-local",
          },
          label: "Cursor",
          availability:
            inventory.unavailable === true
              ? { state: "unavailable", observedAt: "2026-11-05T00:00:00Z", reason: "stopped" }
              : { state: "available", observedAt: "2026-11-05T00:00:00Z" },
          channels:
            inventory.withoutProviderChannel === true
              ? []
              : [
                  {
                    kind: "externalProvider",
                    transport: "stdioAcp",
                    bindingId: STAND_IN_BINDING,
                    bindingGeneration: 2,
                    runtime: { provider: "cursor", runtimeName: "stand-in", runtimeVersion: "1" },
                    capabilities: [],
                  },
                ],
        },
      ],
    };
  }

  private snapshot(operationId: string, stage = "terminal"): Json {
    const kind = this.kinds.get(operationId) ?? "conversationPrompt";
    const load = kind === "conversationLoad" ? (this.options.loadBinding ?? {}) : {};
    return {
      operationId,
      operation: kind,
      binding: {
        kind: "externalProvider",
        binding: {
          bindingId: load.bindingId ?? STAND_IN_BINDING,
          generation: {
            serviceEpoch: load.epoch ?? this.options.bindingEpoch ?? STAND_IN_EPOCH,
            generation: 2,
          },
        },
      },
      target: this.target(lookupSession(this.store, operationId) ?? "session-1"),
      stage,
      effect: stage === "terminal" ? "applied" : "unknown",
      reconciliation: stage === "terminal" ? "confirmed" : "unresolved",
      ...(kind === "conversationPrompt" ? { terminalStopReason: "end_turn" } : {}),
    };
  }

  private refuse(operationId: string, sessionId: string, tool: string): Answer {
    if (this.options.recordRefused === true) this.recordOperation(operationId, tool, sessionId);
    return {
      error: {
        mcpResult: "error",
        kind: "unsupportedCapability",
        message: `provider runtime does not advertise conversation ${tool}`,
        stage: "binding",
        effect: "none",
        operationId,
        target: this.target(sessionId),
      },
    };
  }

  private publish(session: SessionState, events: readonly Json[]): void {
    for (const event of events) {
      session.next += 1n;
      session.events.push({ sequence: session.next, event });
    }
  }

  private openEpoch(
    session: SessionState,
    history: readonly RecordedExchange[],
    stop: unknown,
  ): void {
    session.epoch += 1n;
    session.next = 0n;
    session.events = [];
    this.publish(session, [
      { kind: "settingsChanged", settings: {} },
      { kind: "capabilitiesChanged", capabilities: {} },
    ]);
    history.forEach((exchange, index) =>
      this.publish(session, turnEvents(`history-${session.epoch}-${index}`, exchange, stop)),
    );
    this.publish(session, [{ kind: "stateChanged", state: { kind: "idle" } }]);
  }

  private write(sql: string, ...values: (string | number)[]): void {
    const db = new DatabaseSync(this.store);
    db.prepare(sql).run(...values);
    db.close();
  }

  private recordSession(sessionId: string, createdBy: string = STAND_IN_OWNER): void {
    this.write(
      "INSERT INTO provider_session_records VALUES (?, ?, ?, ?, ?)",
      STAND_IN_SERVICE,
      this.options.endpointId ?? "cursor-local",
      sessionId,
      encode({ humanId: createdBy }),
      encode({ humanId: STAND_IN_OWNER }),
    );
  }

  private recordOperation(operationId: string, kind: string, sessionId: string): void {
    this.kinds.set(operationId, kind);
    this.write(
      "INSERT INTO provider_operations VALUES (?, ?, ?, ?, ?, 'terminal', 'applied', 'confirmed', 1, 2)",
      operationId,
      kind,
      STAND_IN_SERVICE,
      this.options.endpointId ?? "cursor-local",
      sessionId,
    );
  }

  private receiptRow(method: string, sessionId: string): void {
    appendFileSync(this.receipt, `${JSON.stringify({ method, params: { sessionId } })}\n`);
  }
}

export async function startStandIn(options: StandInOptions = {}): Promise<StandIn> {
  const root = mkdtempSync(join(tmpdir(), "mcp-stand-in-"));
  const host = new ScriptedHost(
    options,
    join(root, "provider-operations.sqlite"),
    join(root, "receipt.jsonl"),
  );
  const calls: string[] = [];
  const server: Server = createServer((request, response) => {
    let text = "";
    request.on("data", (chunk: Buffer) => (text += chunk.toString("utf8")));
    request.on("end", () => {
      const call = callSchema.parse(decode(text));
      calls.push(call.params.name);
      const answered = host.answer(call.params.name, call.params.arguments);
      response.setHeader("content-type", "application/json");
      response.end(
        encode({
          jsonrpc: "2.0",
          id: call.id,
          result:
            "error" in answered
              ? { isError: true, structuredContent: answered.error }
              : { structuredContent: answered.result },
        }),
      );
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (address === null || typeof address === "string") throw new Error("no stand-in port");
  return {
    url: `http://127.0.0.1:${address.port}/mcp`,
    store: host.store,
    receipt: host.receipt,
    root,
    calls,
    sessions: host.sessions,
    async close(): Promise<void> {
      server.closeAllConnections();
      await new Promise<void>((resolve) => server.close(() => resolve()));
      rmSync(root, { recursive: true, force: true });
    },
  };
}

function lookupSession(store: string, operationId: string): string | null {
  const db = new DatabaseSync(store, { readOnly: true });
  try {
    const row = z
      .object({ target_session_id: z.string() })
      .optional()
      .parse(
        db
          .prepare("SELECT target_session_id FROM provider_operations WHERE operation_id = ?")
          .get(operationId),
      );
    return row?.target_session_id ?? null;
  } finally {
    db.close();
  }
}
