import { appendFile, lstat } from "node:fs/promises";
import { dirname } from "node:path";
import { pathToFileURL } from "node:url";
import { z } from "zod";
import { sanitize } from "./error-cause.ts";
const output = z.string().startsWith("/").parse(process.env.HARNESS_CAUSE_OUTPUT);
const info = await lstat(dirname(output));
if (!info.isDirectory() || info.uid !== process.getuid?.() || (info.mode & 0o077) !== 0)
  throw new Error("private cause destination required");
let count = 0;
function requestStage(
  arguments_: readonly unknown[],
): "version-upload" | "deployment-activation" | "client-request" {
  const url = z
    .union([z.string(), z.instanceof(URL), z.object({ url: z.string() })])
    .safeParse(arguments_[0]);
  let path = "";
  if (url.success)
    try {
      path = new URL(
        typeof url.data === "string"
          ? url.data
          : url.data instanceof URL
            ? url.data.href
            : url.data.url,
      ).pathname;
    } catch {
      /* A non-URL input cannot label a deployment stage. */
    }
  const options = z
    .object({ method: z.string().optional() })
    .safeParse(arguments_[1] ?? arguments_[0]);
  const method = options.success ? (options.data.method ?? "GET").toUpperCase() : "GET";
  return method === "POST" && path.endsWith("/versions")
    ? "version-upload"
    : method === "POST" && path.endsWith("/deployments")
      ? "deployment-activation"
      : "client-request";
}
async function capture(error: unknown, arguments_: readonly unknown[]): Promise<void> {
  if (count++ >= 32) {
    if (count === 33)
      await appendFile(output, `${JSON.stringify({ kind: "limit", messagesRetained: false })}\n`, {
        mode: 0o600,
      });
    return;
  }
  const stage = requestStage(arguments_);
  await appendFile(
    output,
    `${JSON.stringify({ kind: "error", stage, cause: sanitize(error, new Set()), messagesRetained: false })}\n`,
    { mode: 0o600 },
  );
}
async function captureHttp(result: unknown, arguments_: readonly unknown[]): Promise<void> {
  const response = z
    .object({ ok: z.boolean(), status: z.number().int().min(100).max(599) })
    .safeParse(result);
  if (response.success && !response.data.ok) {
    if (count++ >= 32) {
      if (count === 33)
        await appendFile(
          output,
          `${JSON.stringify({ kind: "limit", messagesRetained: false })}\n`,
          { mode: 0o600 },
        );
      return;
    }
    await appendFile(
      output,
      `${JSON.stringify({ kind: "http", stage: requestStage(arguments_), status: response.data.status, messagesRetained: false })}\n`,
      { mode: 0o600 },
    );
  }
}
const original = globalThis.fetch;
globalThis.fetch = async function captureFetch(
  input: string | URL | Request,
  options?: RequestInit,
): Promise<Response> {
  try {
    const result = await original(input, options);
    await captureHttp(result, [input, options]);
    return result;
  } catch (error: unknown) {
    await capture(error, [input, options]);
    throw error;
  }
};
const modulePath = z.string().startsWith("/").parse(process.env.HARNESS_CF_BUNDLE);
const factoryName = z.string().min(1).parse(process.env.HARNESS_CF_FACTORY);
const exports = z
  .record(z.string(), z.unknown())
  .parse(await import(pathToFileURL(modulePath).href));
const factory = exports[factoryName];
if (typeof factory !== "function") throw new Error("declared deploy Undici factory absent");
const undici: unknown = Reflect.apply(factory, undefined, []);
if (typeof undici !== "object" || undici === null)
  throw new Error("declared bundled Undici absent");
const bundledFetch: unknown = Reflect.get(undici, "fetch");
if (typeof bundledFetch !== "function") throw new Error("declared bundled fetch absent");
Reflect.set(undici, "fetch", async (...arguments_: unknown[]): Promise<unknown> => {
  try {
    const result: unknown = await Reflect.apply(bundledFetch, undici, arguments_);
    await captureHttp(result, arguments_);
    return result;
  } catch (error: unknown) {
    await capture(error, arguments_);
    throw error;
  }
});
