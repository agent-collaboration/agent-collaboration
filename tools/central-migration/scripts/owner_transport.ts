// How the operator CLIs reach a deployed Worker's owner routes, and where each
// answer lands (T2):
//
// - ownerFetch never follows a redirect. Each call carries the owner's Access
//   token, and following a 307 or 308 would resend the method, the body and
//   that token to whatever host the Location names, production included. Any
//   3xx answer is refused instead; Access's own login redirect (no token, or
//   an expired one) is refused the same way.
// - callIntoFile takes the --out file exclusively (created, mode 0600) before
//   the first request, so an existing file refuses the run with nothing sent.
//   The answer, or the reason the call failed, is written into it once.

import { open } from "node:fs/promises";
import { parseExact, stringifyExact } from "../src/exact_json.ts";
import {
  callStep,
  type MigrationStep,
  type MigrationTransport,
  type StepResult,
} from "./migration_call.ts";

export class RedirectRefused extends Error {}
export class OutputExists extends Error {}

/** fetch with the owner token, refusing any redirect rather than following it. */
export async function ownerFetch(url: URL, token: string, init: RequestInit): Promise<Response> {
  const response = await fetch(url, {
    ...init,
    headers: { ...init.headers, "cf-access-token": token },
    redirect: "manual",
  });
  if (response.type === "opaqueredirect" || (response.status >= 300 && response.status < 400)) {
    await response.body?.cancel();
    const location = response.headers.get("location");
    const target = location === null ? "no location" : new URL(location, url).host;
    throw new RedirectRefused(
      `${url.pathname} answered ${response.status} towards ${target}; owner calls never follow a redirect`,
    );
  }
  return response;
}

/** The migration calls' transport against one origin. */
export function ownerTransport(origin: string, token: string): MigrationTransport {
  return async (method, path, body) => {
    const response = await ownerFetch(new URL(path, origin), token, {
      method,
      headers: { "content-type": "application/json" },
      ...(body === undefined ? {} : { body }),
    });
    return { status: response.status, text: await response.text() };
  };
}

/** One step whose answer goes, once and privately, into a file that didn't exist before it. */
export async function callIntoFile(
  out: string,
  step: MigrationStep,
  transport: MigrationTransport,
  body: string | undefined,
): Promise<StepResult> {
  const file = await open(out, "wx", 0o600).catch((error: unknown) => {
    if ((error as { code?: string }).code === "EEXIST")
      throw new OutputExists(`${out} already exists; every answer goes to a new file`);
    throw error;
  });
  try {
    const result = await callStep(step, transport, body, parseExact);
    await file.writeFile(stringifyExact(result.answer));
    return result;
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    await file.writeFile(stringifyExact({ step, failed: reason }));
    throw error;
  } finally {
    await file.close();
  }
}
