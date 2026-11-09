// S4 (T11): one staging observation per row of R7's contradiction table. The
// corpus is the migration's own production fixture and the cases are the
// shared local ones (tools/central-migration/test/support/contradictions.ts),
// so staging runs exactly what local workerd runs. This module computes the
// manifest and runs each phase over a transport, so the staging driver
// (s4_refusals.ts) and the local rehearsal test share every step.

import { CONTRADICTIONS } from "../../../tools/central-migration/test/support/contradictions.ts";
import {
  productionSeed,
  type ProductionSeed,
} from "../../../tools/central-migration/test/support/production_fixture.ts";
import type { ObjectImage, SnapshotPage } from "../src/s4_fixture_storage.ts";

/** R7's rows, each with the shared case that observes it on staging. */
export const S4_ROWS: readonly { readonly row: string; readonly cases: readonly string[] }[] = [
  {
    row: "Central's copy is at a higher lease number than the registry's holder",
    cases: ["the copy is at a higher lease number than the registry"],
  },
  {
    row: "The copy's holder differs from the registry's holder at the same lease number",
    cases: ["the copy's holder differs from the registry's at the same lease number"],
  },
  {
    row: "A project-directory entry with no registry record, or a held registry record with no directory entry",
    cases: [
      "a directory entry has no registry record",
      "a held registry record has no directory entry",
    ],
  },
  {
    row: "A storage shape the migration doesn't recognise",
    cases: ["a storage shape the migration doesn't recognise"],
  },
  { row: "A holder Mac with no enrollment", cases: ["a holder Mac has no enrollment"] },
  {
    row: "Central's configured receiver differs from the one the Mac's sender is registered with",
    cases: [
      "central's configured receiver differs from the one the Mac's sender is registered with",
    ],
  },
  {
    row: "A copy's acknowledged position isn't inside the holder file's retained window",
    cases: ["a copy's acknowledged position isn't inside the holder file's retained window"],
  },
  {
    row: "A holder change, retirement or other lease operation still in progress in the old registry",
    cases: ["a holder change is still in progress"],
  },
  {
    row: "An unfinished journal operation in the old Registry or Fleet object",
    cases: ["an unfinished journal operation in the old Fleet object"],
  },
  {
    row: "Any row in the migration-failure table, or a migration set other than production's full set",
    cases: [
      "a row in the migration-failure table",
      "a migration set other than production's full set",
    ],
  },
  {
    row: "Any carried value that main's types or rules reject",
    cases: ["a carried graph value main's types reject"],
  },
];

/** One object of the cohort, as the old central addresses it. */
export interface CohortObject {
  readonly binding: "LEASE_REGISTRY" | "FLEET" | "PROJECT_COPIES" | "CONNECTIONS" | "MAILBOXES";
  readonly name: string;
}
export interface ObjectStatements extends CohortObject {
  readonly statements: readonly string[];
}
export interface S4Case {
  readonly id: string;
  readonly row: string;
  readonly name: string;
  /** Statements written on top of the seeded baseline, per object. */
  readonly objects: readonly ObjectStatements[];
  /** Migrate's body for this case (its Mac-file facts). */
  readonly migrateBody: { readonly macFiles: ProductionSeed["macFiles"] };
  readonly expected: { readonly object: string; readonly subject: string; readonly reason: string };
}
export interface S4Manifest {
  readonly baseline: readonly ObjectStatements[];
  readonly cases: readonly S4Case[];
}

function cohort(seed: ProductionSeed): ObjectStatements[] {
  return [
    { binding: "LEASE_REGISTRY", name: "registry", statements: seed.registry },
    { binding: "FLEET", name: "fleet", statements: seed.fleet },
    ...Object.entries(seed.copies).map(([name, statements]) => ({
      binding: "PROJECT_COPIES" as const,
      name,
      statements,
    })),
    ...Object.entries(seed.connections).map(([name, statements]) => ({
      binding: "CONNECTIONS" as const,
      name,
      statements,
    })),
    ...Object.entries(seed.mailboxes).map(([name, statements]) => ({
      binding: "MAILBOXES" as const,
      name,
      statements,
    })),
  ];
}

const key = (object: CohortObject) => `${object.binding}/${object.name}`;
const slug = (name: string) =>
  name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");

/** The baseline cohort and each representative case's delta over it. */
export function s4Manifest(): S4Manifest {
  const baseline = cohort(productionSeed());
  const baselineByKey = new Map(baseline.map((object) => [key(object), object]));
  const cases = S4_ROWS.flatMap(({ row, cases: names }) =>
    names.map((name): S4Case => {
      const contradiction = CONTRADICTIONS.find((candidate) => candidate.name === name);
      if (contradiction === undefined) throw new Error(`no shared case named ${name}`);
      const edited = productionSeed();
      contradiction.edit(edited);
      const objects = cohort(edited).flatMap((object): ObjectStatements[] => {
        const before = baselineByKey.get(key(object))?.statements ?? [];
        const prefix = object.statements.slice(0, before.length);
        if (prefix.join("\n") !== before.join("\n"))
          throw new Error(`case ${name} rewrites ${key(object)}; S4 cases may only append`);
        const added = object.statements.slice(before.length);
        return added.length === 0 ? [] : [{ ...object, statements: added }];
      });
      return {
        id: slug(name),
        row,
        name,
        objects,
        migrateBody: { macFiles: edited.macFiles },
        expected: {
          object: contradiction.object,
          subject: contradiction.subject,
          reason: contradiction.reason.source,
        },
      };
    }),
  );
  return { baseline, cases };
}

/** One owner call to the S4 seed routes: path and JSON body in, status and JSON out. */
export type S4Call = (path: string, body: unknown) => Promise<{ status: number; body: unknown }>;

async function ok(call: S4Call, path: string, body: unknown): Promise<unknown> {
  const answer = await call(path, body);
  if (answer.status !== 200)
    throw new Error(`${path} answered ${answer.status}: ${JSON.stringify(answer.body)}`);
  return answer.body;
}

/** Runs statements in each object, one transaction per object. */
export async function writeStatements(
  call: S4Call,
  objects: readonly ObjectStatements[],
): Promise<number> {
  let rowsWritten = 0;
  for (const object of objects) {
    const answer = (await ok(call, "/admin/v1/staging-seed/s4/statements", {
      binding: object.binding,
      name: object.name,
      payload: object.statements,
    })) as { rowsWritten: number };
    rowsWritten += answer.rowsWritten;
  }
  return rowsWritten;
}

/** One object's full image, every page, with no write. */
export async function snapshotObject(call: S4Call, object: CohortObject): Promise<ObjectImage> {
  let cursor: SnapshotPage["next"] = null;
  let schema: ObjectImage["schema"] = [];
  const tables: Record<
    string,
    { columns: readonly string[]; rows: ObjectImage["tables"][string]["rows"][number][] }
  > = {};
  for (let page = 0; ; page += 1) {
    const answer = (await ok(call, "/admin/v1/staging-seed/s4/snapshot", {
      ...object,
      payload: cursor,
    })) as SnapshotPage;
    if (answer.rowsWritten !== 0) throw new Error(`snapshot of ${key(object)} wrote rows`);
    if (page === 0) schema = answer.schema ?? [];
    if (answer.table !== null) {
      const table = (tables[answer.table] ??= { columns: answer.columns, rows: [] });
      table.rows.push(...answer.rows);
    }
    if (answer.next === null) return { schema, tables };
    cursor = answer.next;
  }
}

/** Every cohort object's image, keyed binding/name. */
export async function snapshotCohort(
  call: S4Call,
  objects: readonly CohortObject[],
): Promise<Record<string, ObjectImage>> {
  const images: Record<string, ObjectImage> = {};
  for (const object of objects) images[key(object)] = await snapshotObject(call, object);
  return images;
}

/** Restores each object a case touched to its saved image. */
export async function restoreObjects(
  call: S4Call,
  objects: readonly CohortObject[],
  images: Readonly<Record<string, ObjectImage>>,
): Promise<number> {
  let rowsWritten = 0;
  for (const object of objects) {
    const image = images[key(object)];
    if (image === undefined) throw new Error(`no saved image for ${key(object)}`);
    const answer = (await ok(call, "/admin/v1/staging-seed/s4/restore", {
      ...object,
      payload: image,
    })) as { rowsWritten: number };
    rowsWritten += answer.rowsWritten;
  }
  return rowsWritten;
}

export const cohortKey = key;
