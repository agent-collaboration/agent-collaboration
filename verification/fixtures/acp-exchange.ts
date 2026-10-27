import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { AcpStdio, wireJson } from "./acp-stdio.ts";
import { field } from "../exact-json.ts";

export class AcpExchange extends AcpStdio {
  async read(method?: string): Promise<Record<string, unknown>> {
    const request = await this.readRequest();
    assert.notEqual(request, undefined, "expected ACP request before EOF");
    if (request === undefined) throw new Error("expected ACP request before EOF");
    if (method !== undefined) assert.equal(request.method, method);
    return request;
  }
  respond(request: Readonly<Record<string, unknown>>, result: unknown): void {
    this.send({ jsonrpc: "2.0", id: request.id, result });
  }
  update(update: unknown, sessionId: string = "fixture-session"): void {
    this.send({ jsonrpc: "2.0", method: "session/update", params: { sessionId, update } });
  }
  async initialize(name: string, result: Readonly<Record<string, unknown>> = {}): Promise<void> {
    this.respond(await this.read("initialize"), {
      protocolVersion: 1,
      agentCapabilities: {},
      agentInfo: { name, version: "1" },
      ...result,
    });
  }
  async newSession(
    sessionId: string = "fixture-session",
    extra: Readonly<Record<string, unknown>> = {},
  ): Promise<void> {
    this.respond(await this.read("session/new"), { sessionId, ...extra });
  }
  receipt(path: string, value: unknown): void {
    writeFileSync(path, wireJson(value));
  }
  endTurn(request: Readonly<Record<string, unknown>>, stopReason: string = "end_turn"): void {
    this.respond(request, { stopReason });
  }
  async responseReceipt(path: string, id: unknown, resultOnly: boolean = false): Promise<void> {
    const response = await this.read();
    assert.deepEqual(response.id, id);
    this.receipt(path, resultOnly ? field(response, "result") : response);
  }
}
