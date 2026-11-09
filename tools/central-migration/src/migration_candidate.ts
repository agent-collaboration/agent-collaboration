// The candidate: every object exactly as migrate would leave it, rehearsed in
// each object's own rolled-back transaction, so nothing is kept. Its canonical
// text is what the Rust decode gate decodes; its SHA-256 is what the gate's
// pass names and what migrate must reproduce, from the same run time and the
// same old storage, before its first write (PD7 between inventory and migrate).

import { stringifyExact } from "./exact_json.ts";
import type { MigrationPlan, OldStorage } from "./migration_plan.ts";
import { hex } from "./schema_sets.ts";
import type { ObjectImage } from "./storage_objects.ts";

export interface Candidate {
  readonly fleet: ObjectImage;
  /** Each Copy without its project tables, keyed by project, in sorted order. */
  readonly copies: Readonly<Record<string, ObjectImage>>;
}

export interface CandidateSource {
  rehearseFleet(statements: MigrationPlan["fleet"]): Promise<ObjectImage>;
  rehearseCopy(
    project: string,
    statements: MigrationPlan["fleet"],
    exclude: readonly string[],
  ): Promise<ObjectImage>;
}

/** Which objects already hold main's records: those rehearse with no statements. */
export interface MigratedState {
  readonly fleet: boolean;
  readonly copies: Readonly<Record<string, boolean>>;
}

export async function rehearseCandidate(
  source: CandidateSource,
  plan: MigrationPlan,
  old: OldStorage,
  state: MigratedState,
): Promise<{ readonly text: string; readonly digest: string }> {
  const fleet = await source.rehearseFleet(state.fleet ? [] : plan.fleet);
  const copies: Record<string, ObjectImage> = {};
  for (const project of Object.keys(old.copies).sort()) {
    const projectTables = (old.copies[project]?.rows._fleet_sync_copy_tables ?? []).map((row) =>
      String(row.name),
    );
    copies[project] = await source.rehearseCopy(
      project,
      state.copies[project] === true ? [] : (plan.copies[project] ?? []),
      projectTables,
    );
  }
  const text = stringifyExact({ fleet, copies } satisfies Candidate);
  return { text, digest: await sha256Hex(text) };
}

export async function sha256Hex(text: string): Promise<string> {
  return hex(new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text))));
}
