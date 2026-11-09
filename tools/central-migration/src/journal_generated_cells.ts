// The journal cells main's own SQL generates while converting an object.
// agent-fleet-sync's durable-steps 0002 (f052831) adds journal metadata and
// fills it for existing rows from the clock (unixepoch('subsec')) and from
// random(); 0003 then drops the one other table it generates. The migration
// runs that SQL unmodified, so these cells differ on every run:
//
// - the candidate replaces each with a deterministic stand-in, so inventory's
//   rehearsal and migrate's rebuild hash alike, and every other cell, including
//   every carried column of the same rows, stays bound by the digest;
// - verify checks the cells migrate actually wrote for shape: the value 0002
//   derives from carried columns where it does, and otherwise a time (or a
//   UUIDv7's time) inside the run window, from the object's stamp to verify.

import type { Check } from "./migration_verify.ts";
import type { ObjectImage, ReadReply, ReadRow, TypedValue } from "./storage_objects.ts";

/** The only masked cells: each table and the columns 0002 generates in it. */
export const GENERATED_JOURNAL_CELLS = {
  _fleet_sync_operations: ["created_at", "updated_at"],
  _fleet_sync_steps: ["id", "created_at", "updated_at"],
} as const satisfies Readonly<Record<string, readonly string[]>>;

/** A masked cell: the stand-in the decode gate inserts, so NOT NULL and keys still hold. */
export type GeneratedCell = { readonly kind: "generated"; readonly value: TypedValue };
export type CandidateValue = TypedValue | GeneratedCell;
export interface CandidateImage {
  readonly schema: ObjectImage["schema"];
  readonly tables: Readonly<
    Record<
      string,
      { readonly columns: readonly string[]; readonly rows: readonly (readonly CandidateValue[])[] }
    >
  >;
}

const standIn = (table: string, column: string, row: number): TypedValue =>
  column === "id"
    ? { kind: "text", value: `generated-${table}-${row}` }
    : { kind: "integer", value: "0" };

/** The image with exactly GENERATED_JOURNAL_CELLS replaced by stand-ins. */
export function maskGenerated(image: ObjectImage): CandidateImage {
  const tables: Record<string, CandidateImage["tables"][string]> = {};
  for (const [table, contents] of Object.entries(image.tables)) {
    const generated: readonly string[] =
      (GENERATED_JOURNAL_CELLS as Readonly<Record<string, readonly string[]>>)[table] ?? [];
    const masked = new Set(
      contents.columns.flatMap((column, index) => (generated.includes(column) ? [index] : [])),
    );
    tables[table] = {
      columns: contents.columns,
      rows: contents.rows.map((row, rowIndex) =>
        row.map((value, index): CandidateValue => {
          const column = contents.columns[index] ?? "";
          return masked.has(index)
            ? { kind: "generated", value: standIn(table, column, rowIndex) }
            : value;
        }),
      ),
    };
  }
  return { schema: image.schema, tables };
}

const UUID_V7 = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const integerOf = (value: ReadRow[string] | undefined): number | null =>
  typeof value === "number" && Number.isSafeInteger(value) ? value : null;
const uuidV7Millis = (id: string): number =>
  Number.parseInt(id.replaceAll("-", "").slice(0, 12), 16);

/**
 * The generated cells of one converted object, for shape. `window` is the run:
 * from the object's whole-second stamp (before its write) to verify's time.
 */
export function generatedJournalChecks(
  name: string,
  object: ReadReply,
  window: { readonly from: number; readonly to: number },
): Check[] {
  const inWindow = (time: number) => time >= window.from && time <= window.to;
  // 0002: created = coalesce(carried times..., now); updated = max(created, now).
  const created = (value: number | null, derived: number | null) =>
    value !== null && (derived !== null ? value === derived : inWindow(value));
  const updated = (value: number | null, createdAt: number | null) =>
    value !== null &&
    createdAt !== null &&
    // max(created, t) for some t in the window: t itself, or created when it is later.
    ((inWindow(value) && value >= createdAt) || (value === createdAt && createdAt >= window.from));
  const checks: Check[] = [];
  const operations = object.rows._fleet_sync_operations ?? [];
  const steps = object.rows._fleet_sync_steps ?? [];
  const operationCreated = new Map<string, number | null>();
  for (const operation of operations) {
    const id = String(operation.operation_id);
    const firstAttempts = steps
      .filter((step) => String(step.operation_id) === id)
      .map((step) => integerOf(step.first_attempt_at))
      .filter((time): time is number => time !== null);
    const derived =
      firstAttempts.length > 0
        ? Math.min(...firstAttempts)
        : (integerOf(operation.finished_at) ?? integerOf(operation.next_attempt_at));
    const createdAt = integerOf(operation.created_at);
    operationCreated.set(id, createdAt);
    checks.push({
      name: `${name} journal operation ${id} times`,
      ok: created(createdAt, derived) && updated(integerOf(operation.updated_at), createdAt),
      detail: `created_at ${String(operation.created_at)}, updated_at ${String(operation.updated_at)}, window ${window.from}..${window.to}`,
    });
  }
  for (const step of steps) {
    const id = String(step.id);
    const derived =
      integerOf(step.first_attempt_at) ?? operationCreated.get(String(step.operation_id)) ?? null;
    const createdAt = integerOf(step.created_at);
    checks.push({
      name: `${name} journal step ${String(step.operation_id)}/${String(step.position)}`,
      ok:
        UUID_V7.test(id) &&
        inWindow(uuidV7Millis(id)) &&
        created(createdAt, derived) &&
        updated(integerOf(step.updated_at), createdAt),
      detail: `id ${id}, created_at ${String(step.created_at)}, updated_at ${String(step.updated_at)}, window ${window.from}..${window.to}`,
    });
  }
  return checks;
}
