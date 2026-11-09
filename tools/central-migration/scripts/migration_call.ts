// The migration Worker's four owner calls as PD7 runs them (T2): what each
// sends and what counts as success. Pure: the transport is injected.
//
// - inventory: POST with the Mac-file body; success is no stop and nothing
//   written. Its answer holds the only copy of the retiring classes.
// - migrate: POST with the facts plus the run time and candidate digest the
//   Rust decode gate passed (migrateBody); success is `migrated`, or
//   `alreadyMigrated` on a rerun. A `stopped` answer wrote nothing.
// - verify: GET; success is `ok: true`.
// - revert: POST; success is `reverted`.

import { parseExact, stringifyExact } from "../src/exact_json.ts";

export type MigrationStep = "inventory" | "migrate" | "verify" | "revert";
export type MigrationTransport = (
  method: "GET" | "POST",
  path: string,
  body: string | undefined,
) => Promise<{ status: number; text: string }>;

export interface StepResult {
  readonly step: MigrationStep;
  readonly status: number;
  readonly answer: unknown;
  readonly success: boolean;
  readonly reason: string;
}

const ROUTES: Readonly<Record<MigrationStep, { method: "GET" | "POST"; body: boolean }>> = {
  inventory: { method: "POST", body: true },
  migrate: { method: "POST", body: true },
  verify: { method: "GET", body: false },
  revert: { method: "POST", body: false },
};

/** Whether one step's answer is the success PD7 requires, and why not. */
export function judge(
  step: MigrationStep,
  status: number,
  answer: unknown,
): { success: boolean; reason: string } {
  const body = (answer ?? {}) as {
    kind?: string;
    stops?: unknown[];
    rowsWritten?: number;
    ok?: boolean;
  };
  switch (step) {
    case "inventory":
      if (status !== 200 || body.kind !== "inventory")
        return { success: false, reason: `inventory answered ${status} ${String(body.kind)}` };
      if ((body.stops ?? []).length > 0)
        return { success: false, reason: `${body.stops?.length} stops; migrate must not run` };
      if (body.rowsWritten !== 0)
        return { success: false, reason: `the dry run wrote ${String(body.rowsWritten)} rows` };
      return { success: true, reason: "no stops, nothing written" };
    case "migrate":
      if (status === 200 && (body.kind === "migrated" || body.kind === "alreadyMigrated"))
        return { success: true, reason: String(body.kind) };
      return { success: false, reason: `migrate answered ${status} ${String(body.kind)}` };
    case "verify":
      return status === 200 && body.ok === true
        ? { success: true, reason: "every check passed" }
        : { success: false, reason: `verify answered ${status}, ok ${String(body.ok)}` };
    case "revert":
      return status === 200 && body.kind === "reverted"
        ? { success: true, reason: "reverted" }
        : { success: false, reason: `revert answered ${status} ${String(body.kind)}` };
  }
}

/** The Rust decode gate's report, as migrate needs it. */
export interface GateReport {
  readonly outcome: string;
  readonly candidateDigest?: string;
  readonly runTimeMillis?: number;
}

/**
 * Migrate's body: refuses anything but the gate's pass, which names the digest
 * and run time. The Mac-file facts pass through the exact codec, so leases and
 * positions beyond 2^53 reach the Worker unchanged.
 */
export function migrateBody(macFilesBody: string, gate: GateReport): string {
  if (
    gate.outcome !== "pass" ||
    typeof gate.candidateDigest !== "string" ||
    typeof gate.runTimeMillis !== "number"
  ) {
    throw new Error(
      "migrate runs only after the decode gate's pass, which names the candidate digest and run time",
    );
  }
  const { macFiles } = parseExact(macFilesBody) as { macFiles: unknown };
  return stringifyExact({
    macFiles,
    runTime: gate.runTimeMillis,
    candidateDigest: gate.candidateDigest,
  });
}

/** One call: the step's route and body, and the judged answer. Answers parse exactly (big integers kept). */
export async function callStep(
  step: MigrationStep,
  transport: MigrationTransport,
  body: string | undefined,
  parse: (text: string) => unknown,
): Promise<StepResult> {
  const route = ROUTES[step];
  if (route.body && body === undefined) throw new Error(`${step} needs its body`);
  const response = await transport(
    route.method,
    `/admin/v1/migration/${step}`,
    route.body ? body : undefined,
  );
  let answer: unknown;
  try {
    answer = parse(response.text);
  } catch {
    answer = { unparsed: response.text.slice(0, 300) };
  }
  return { step, status: response.status, answer, ...judge(step, response.status, answer) };
}
