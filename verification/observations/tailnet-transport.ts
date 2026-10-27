import { spawn } from "node:child_process";
import { request } from "node:http";
import { z } from "zod";
import { evidenceJson } from "../evidence-json.ts";
import { sameJsonContent } from "../json-comparison.ts";
import { observedObject, readObservedJson, type JsonObject } from "./evidence-models.ts";
import { processExit, ProcessWaitTimeout } from "./runtime-plan.ts";
import { httpsAuthority, type HttpsEndpoint } from "./tailnet-plans.ts";

export interface HttpReply {
  readonly status: number;
  readonly body: Buffer;
}
export interface ProbeHeaders {
  readonly origin?: string | null;
  readonly host_authority?: string | null;
}
export type HttpTarget =
  | { readonly kind: "https"; readonly endpoint: HttpsEndpoint }
  | { readonly kind: "unix"; readonly path: string }
  | { readonly kind: "loopback"; readonly url: string };

let systemCurlQualified = false;

async function curlCommand(
  arguments_: readonly string[],
  body?: Buffer,
): Promise<{ readonly exit: number; readonly stdout: Buffer; readonly stderr: Buffer }> {
  const environment = { ...process.env };
  for (const name of body === undefined
    ? []
    : ["CURL_CA_BUNDLE", "SSL_CERT_FILE", "SSL_CERT_DIR", "SSLKEYLOGFILE", "CURL_SSL_BACKEND"])
    delete environment[name];
  return new Promise((resolve, reject): void => {
    const child = spawn("/usr/bin/curl", arguments_, {
      env: environment,
      stdio: ["pipe", "pipe", "pipe"],
    });
    const stdout: Buffer[] = [];
    const stderr: Buffer[] = [];
    let timedOut = false;
    const timer = setTimeout(
      (): void => {
        timedOut = true;
        child.kill("SIGKILL");
      },
      body === undefined ? 5000 : 76_000,
    );
    child.stdout.on("data", (part: Buffer): void => {
      stdout.push(part);
    });
    child.stderr.on("data", (part: Buffer): void => {
      stderr.push(part);
    });
    child.stdin.on("error", (): void => {
      /* curl's exit and captured stderr establish failure. */
    });
    child.on("error", (error): void => {
      clearTimeout(timer);
      reject(error);
    });
    child.on("close", (): void => {
      clearTimeout(timer);
      const exit = processExit(child);
      if (timedOut) reject(new ProcessWaitTimeout("HTTPS curl subprocess timeout"));
      else if (exit === null) reject(new Error("curl exit unavailable"));
      else resolve({ exit, stdout: Buffer.concat(stdout), stderr: Buffer.concat(stderr) });
    });
    child.stdin.end(body);
  });
}

export async function requireSystemCurl(): Promise<void> {
  if (systemCurlQualified) return;
  const result = await curlCommand(["-q", "--version"]);
  const match = /^curl (\d+)\.(\d+)\.(\d+) /.exec(result.stdout.toString());
  if (
    result.exit !== 0 ||
    match === null ||
    Number(match[1]) < 8 ||
    (Number(match[1]) === 8 && Number(match[2]) < 4)
  )
    throw new Error("system curl >=8.4 required for bounded unknown-length bodies");
  systemCurlQualified = true;
}

export function curlPostArguments(
  target: HttpsEndpoint,
  headers: Readonly<Record<string, string>>,
): string[] {
  const arguments_ = [
    "/usr/bin/curl",
    "-q",
    "--silent",
    "--show-error",
    "--globoff",
    "--http1.1",
    "--proto",
    "=https",
    "--noproxy",
    "*",
    "--proxy",
    "",
    "--no-location",
    "--max-redirs",
    "0",
    "--retry",
    "0",
    "--connect-timeout",
    "65",
    "--max-time",
    "75",
    "--max-filesize",
    String(4 * 1024 * 1024),
    "--request",
    "POST",
    "--data-binary",
    "@-",
    "--header",
    "User-Agent:",
    "--header",
    "Expect:",
    "--write-out",
    "\n%{http_code}",
    "--url",
    target.url,
  ];
  for (const [name, value] of Object.entries(headers)) {
    if (
      !/^[A-Za-z0-9-]+$/.test(name) ||
      [...value].some((character) => [0, 10, 13].includes(character.charCodeAt(0)))
    )
      throw new Error("invalid HTTP probe header");
    arguments_.push("--header", `${name}: ${value}`);
  }
  return arguments_;
}
export function curlHttpReply(completed: {
  readonly exit: number;
  readonly stdout: Buffer;
  readonly stderr: Buffer;
}): HttpReply {
  if (completed.exit !== 0)
    throw new Error(
      `HTTPS curl failed (exit ${completed.exit}): ${completed.stderr.subarray(0, 4096).toString("utf8")}`,
    );
  const split = completed.stdout.lastIndexOf(10);
  const body = completed.stdout.subarray(0, split);
  const status = completed.stdout.subarray(split + 1).toString();
  if (split === -1 || !/^[1-5][0-9]{2}$/.test(status))
    throw new Error("HTTPS curl returned no valid HTTP status");
  if (body.length > 4 * 1024 * 1024) throw new Error("HTTP response exceeded evidence bound");
  return { status: Number(status), body };
}

function localHttpPost(
  target: Exclude<HttpTarget, { readonly kind: "https" }>,
  body: Buffer,
  headers: Readonly<Record<string, string>>,
): Promise<HttpReply> {
  return new Promise((resolve, reject): void => {
    const url = target.kind === "unix" ? new URL("http://localhost/mcp") : new URL(target.url);
    const connection = request(
      url,
      {
        method: "POST",
        path: "/mcp",
        socketPath: target.kind === "unix" ? target.path : undefined,
        agent: false,
        headers: { ...headers, "Content-Length": body.length },
      },
      (response): void => {
        const chunks: Buffer[] = [];
        let bytes = 0;
        response.on("data", (chunk: Buffer): void => {
          bytes += chunk.length;
          if (bytes > 4 * 1024 * 1024) {
            const error = new Error("HTTP response exceeded evidence bound");
            reject(error);
            response.destroy();
          } else chunks.push(chunk);
        });
        response.on("error", reject);
        response.on("end", (): void => {
          if (response.statusCode === undefined) reject(new Error("HTTP status missing"));
          else resolve({ status: response.statusCode, body: Buffer.concat(chunks) });
        });
      },
    );
    connection.setTimeout(5000, (): void => {
      connection.destroy(new Error("local MCP HTTP timeout"));
    });
    connection.on("error", reject);
    connection.end(body);
  });
}

export async function httpPost(
  target: HttpTarget,
  payload: JsonObject,
  probes?: ProbeHeaders | null,
): Promise<HttpReply> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    Accept: "application/json, text/event-stream",
    "MCP-Protocol-Version": "2025-11-25",
  };
  if (target.kind === "https") headers.Host = httpsAuthority(target.endpoint);
  if (probes?.origin !== undefined && probes.origin !== null) headers.Origin = probes.origin;
  if (probes?.host_authority !== undefined && probes.host_authority !== null)
    headers.Host = probes.host_authority;
  const body = Buffer.from(evidenceJson(payload));
  if (target.kind !== "https") return localHttpPost(target, body, headers);
  await requireSystemCurl();
  return curlHttpReply(
    await curlCommand(curlPostArguments(target.endpoint, headers).slice(1), body),
  );
}

export function rpcResult(reply: HttpReply): JsonObject {
  const envelope = readObservedJson(reply.body);
  if (
    reply.status !== 200 ||
    envelope.jsonrpc !== "2.0" ||
    !sameJsonContent(envelope.id, 1n) ||
    Object.hasOwn(envelope, "error")
  )
    throw new Error("MCP HTTP/error/correlation failure");
  const result = observedObject(envelope.result);
  if (result.isError === true) throw new Error("MCP tool returned an error; no mutation replay");
  return result;
}
export function toolPayload(name: string, arguments_: JsonObject): JsonObject {
  return { jsonrpc: "2.0", id: 1n, method: "tools/call", params: { name, arguments: arguments_ } };
}
export function toolResult(reply: HttpReply): JsonObject {
  return observedObject(rpcResult(reply).structuredContent);
}
export async function tool(
  target: HttpsEndpoint | string,
  name: string,
  arguments_: JsonObject,
): Promise<JsonObject> {
  return toolResult(
    await httpPost(
      typeof target === "string"
        ? { kind: "unix", path: target }
        : { kind: "https", endpoint: target },
      toolPayload(name, arguments_),
    ),
  );
}

export function requireService(
  inventory: JsonObject,
  serviceId: string,
  serviceEpoch?: string | null,
): void {
  if (
    serviceEpoch !== undefined &&
    serviceEpoch !== null &&
    inventory.serviceEpoch !== serviceEpoch
  )
    throw new Error("destination service epoch changed");
  const endpoints = z.array(z.record(z.string(), z.unknown())).parse(inventory.endpoints);
  if (endpoints.length === 0) throw new Error("endpoint inventory absent");
  for (const entry of endpoints)
    if (observedObject(entry.endpoint).serviceId !== serviceId)
      throw new Error("wrong destination service");
  const native = endpoints.filter((entry) =>
    sameJsonContent(entry.endpoint, { serviceId, endpointId: "codex-local" }),
  );
  const entry = native[0];
  if (
    native.length !== 1 ||
    entry === undefined ||
    observedObject(entry.availability).state !== "available"
  )
    throw new Error("native endpoint is not available");
  const channels = z.array(z.unknown()).parse(entry.channels);
  if (
    !channels.some((channel) => {
      const parsed = z.record(z.string(), z.unknown()).safeParse(channel);
      return (
        parsed.success &&
        parsed.data.kind === "nativeCodex" &&
        z.record(z.string(), z.unknown()).safeParse(parsed.data.generation).success
      );
    })
  )
    throw new Error("native generation missing");
  for (const channel of channels) {
    const parsed = z.record(z.string(), z.unknown()).safeParse(channel);
    if (parsed.success && parsed.data.kind === "nativeCodex") {
      const generation = z
        .looseObject({ serviceEpoch: z.string(), generation: z.bigint() })
        .parse(parsed.data.generation);
      if (
        serviceEpoch !== undefined &&
        serviceEpoch !== null &&
        generation.serviceEpoch !== serviceEpoch
      )
        throw new Error("native endpoint epoch differs from the declared Host");
    }
  }
}
