import { request as httpRequest } from "node:http";
import { request as httpsRequest } from "node:https";
import { dirname, join } from "node:path";
import { evidenceJson } from "../evidence-json.ts";
import { decodeRpcBody } from "./callback-oracles.ts";
import {
  observedObject,
  readObservedJson,
  rpcEnvelopeSchema,
  type JsonObject,
} from "./evidence-models.ts";
import { manifestFor, type JourneyPlan } from "./provider-journey-plan.ts";
import { registerGuardIdentifiers } from "./readonly-codex-home.ts";
import { errorCategory, saveRecord } from "./runtime-plan.ts";

function requestBody(url: URL, payload: Buffer): Promise<Buffer> {
  return new Promise((resolve, reject): void => {
    const request = (url.protocol === "https:" ? httpsRequest : httpRequest)(
      url,
      {
        method: "POST",
        agent: false,
        headers: {
          "Content-Type": "application/json",
          Accept: "application/json, text/event-stream",
          "MCP-Protocol-Version": "2025-11-25",
          "Content-Length": payload.length,
          Connection: "close",
        },
      },
      (response): void => {
        const chunks: Buffer[] = [];
        response.on("data", (chunk: Buffer): void => {
          chunks.push(chunk);
        });
        response.on("error", reject);
        response.on("end", (): void => {
          if (
            response.statusCode === undefined ||
            response.statusCode < 200 ||
            response.statusCode >= 300
          )
            reject(new Error(`MCP HTTP ${response.statusCode}; no replay`));
          else resolve(Buffer.concat(chunks));
        });
      },
    );
    request.setTimeout(140_000, (): void => {
      request.destroy(new Error("MCP request timed out; no replay"));
    });
    request.on("error", reject);
    request.end(payload);
  });
}

export async function rpcCall(
  plan: JourneyPlan,
  name: string,
  arguments_: JsonObject,
): Promise<JsonObject> {
  const payload = Buffer.from(
    evidenceJson({
      jsonrpc: "2.0",
      id: 1,
      method: "tools/call",
      params: { name, arguments: arguments_ },
    }),
  );
  const body = await requestBody(new URL(manifestFor(plan).mcp.url), payload);
  const result = rpcEnvelopeSchema.parse(
    decodeRpcBody(new TextDecoder("utf-8", { fatal: true }).decode(body)),
  );
  if (result.error !== null) throw new Error(`RPC ${name} error; no replay: ${result.error.code}`);
  if (result.result === null) throw new Error(`RPC ${name} has no result; no replay`);
  const value = result.result;
  const text = value.content.find((item) => item.type === "text" && item.text !== null)?.text;
  const structured =
    value.structuredContent ??
    (text === undefined || text === null ? undefined : observedObject(readObservedJson(text)));
  if (structured === undefined) throw new Error(`RPC ${name} has no structured result; no replay`);
  if (value.isError || structured.mcpResult === "error")
    throw new Error(`Tool ${name} rejected: ${structured.kind}; no replay`);
  return structured;
}

export async function callOnce(
  plan: JourneyPlan,
  name: string,
  arguments_: JsonObject,
): Promise<JsonObject> {
  registerGuardIdentifiers(dirname(plan.evidence), arguments_);
  saveRecord(join(plan.evidence, `${name}-intent.json`), {
    at: new Date().toISOString().replace("Z", "+00:00"),
    arguments: arguments_,
    testAutomation: true,
  });
  let result;
  try {
    result = await rpcCall(plan, name, arguments_);
  } catch (error) {
    saveRecord(join(plan.evidence, `${name}-unknown.json`), {
      error: errorCategory(error),
      doNotReplay: true,
    });
    throw error;
  }
  saveRecord(join(plan.evidence, `${name}-result.json`), result);
  registerGuardIdentifiers(dirname(plan.evidence), result);
  return result;
}
