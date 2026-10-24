import { z } from "zod";
import { decode, encode } from "../exact-json.ts";

export class CentralOwner {
  readonly origin: URL;
  private readonly headers: Readonly<Record<string, string>>;
  constructor(origin: string, headers: Readonly<Record<string, string>>) {
    this.origin = new URL(origin);
    this.headers = headers;
  }
  async response(input: {
    path: string;
    method?: string;
    body?: unknown;
    authenticated?: boolean;
  }): Promise<{ status: number; headers: Headers; body: unknown }> {
    if (!input.path.startsWith("/admin/v1/") || input.path.startsWith("//"))
      throw new Error("owner path outside admin routes");
    const options: RequestInit = {
      method: input.method ?? "GET",
      redirect: "error",
      signal: AbortSignal.timeout(30_000),
      headers: {
        "Content-Type": "application/json",
        "User-Agent": "codex-router-verification/1.0",
        ...(input.authenticated === false ? {} : this.headers),
      },
    };
    if (input.body !== undefined) options.body = encode(input.body);
    const reply = await fetch(new URL(input.path, this.origin), options);
    const text = await reply.text();
    if (Buffer.byteLength(text) > 16 * 1024 * 1024) throw new Error("owner response exceeds bound");
    return {
      status: reply.status,
      headers: reply.headers,
      body: text === "" ? null : decode(text),
    };
  }
  async call(path: string, body?: unknown): Promise<unknown> {
    const result = await this.response(
      body === undefined ? { path } : { path, method: "POST", body },
    );
    if (result.status !== 200) throw new Error(`owner route returned ${result.status}`);
    return result.body;
  }
  async control(address: unknown, command: unknown): Promise<unknown> {
    return this.call("/admin/v1/conformance/runtime", { address, command });
  }
}
export function environmentValue(name: string): string {
  return z.string().min(1).parse(process.env[name]);
}
