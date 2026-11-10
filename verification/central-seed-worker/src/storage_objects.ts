// The five storage classes under staging central's names, so the seed writes
// the exact objects the old central then opens. Each object accepts only this
// Worker's internal calls, writes only into fresh storage, and does every
// write in one storage transaction.

import { copyRecordJson, publishSnapshot, schemaStepJson } from "./copy_publication.ts";
import type { DurableObjectState, SqlStorage, SqlValue } from "./platform.ts";
import { SeedRefused, type SeedProject } from "./seed_contract.ts";
import type { Statement } from "./seed_storage.ts";
import {
  restore,
  runStatements,
  snapshotPage,
  type ObjectImage,
  type SnapshotCursor,
} from "./s4_fixture_storage.ts";

const INTERNAL = "https://central-seed.internal";
export const OBJECT_STATEMENTS_URL = `${INTERNAL}/statements`;
export const OBJECT_COPY_URL = `${INTERNAL}/copy`;
export const OBJECT_TABLES_URL = `${INTERNAL}/tables`;
export const OBJECT_COPY_RECORD_URL = `${INTERNAL}/copy-record`;
/** S4 refusal allocation only (s4_routes.ts guards every call that reaches these). */
export const S4_STATEMENTS_URL = `${INTERNAL}/s4/statements`;
export const S4_SNAPSHOT_URL = `${INTERNAL}/s4/snapshot`;
export const S4_RESTORE_URL = `${INTERNAL}/s4/restore`;

export interface CopyRequest {
  readonly prelude: readonly Statement[];
  readonly project: SeedProject;
}
export type ObjectReply =
  | { readonly kind: "seeded"; readonly counts?: Readonly<Record<string, number>> }
  | { readonly kind: "refused"; readonly reason: string };

const json = (value: unknown, status = 200): Response =>
  new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json" },
  });

function userTables(sql: SqlStorage): string[] {
  return sql
    .exec<{ name: string }>(
      "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT GLOB '_cf_*' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .toArray()
    .map((row) => row.name);
}

abstract class SeedStorageObject {
  protected readonly state: DurableObjectState;
  constructor(state: DurableObjectState, _env: unknown) {
    this.state = state;
  }

  async fetch(request: Request): Promise<Response> {
    const sql = this.state.storage.sql;
    switch (`${request.method} ${request.url}`) {
      case `GET ${OBJECT_TABLES_URL}`:
        return json(userTables(sql));
      case `GET ${OBJECT_COPY_RECORD_URL}`: {
        if (!userTables(sql).includes("_fleet_sync_copy")) return json(null);
        const [row] = sql
          .exec<{ record: string }>("SELECT record FROM _fleet_sync_copy WHERE singleton = 1")
          .toArray();
        return json(row?.record ?? null);
      }
      case `POST ${OBJECT_STATEMENTS_URL}`:
        return json(this.seed(sql, (await request.json()) as readonly Statement[], undefined));
      case `POST ${OBJECT_COPY_URL}`: {
        const copy = (await request.json()) as CopyRequest;
        return json(this.seed(sql, copy.prelude, copy.project));
      }
      case `POST ${S4_STATEMENTS_URL}`:
        return json(runStatements(this.state, (await request.json()) as readonly string[]));
      case `POST ${S4_SNAPSHOT_URL}`:
        return json(snapshotPage(sql, (await request.json()) as SnapshotCursor | null));
      case `POST ${S4_RESTORE_URL}`:
        return json(restore(this.state, (await request.json()) as ObjectImage));
      default:
        return json({ refused: "route" }, 404);
    }
  }

  /** Fresh storage only; the whole seed lands, or nothing does. */
  private seed(
    sql: SqlStorage,
    statements: readonly Statement[],
    project: SeedProject | undefined,
  ): ObjectReply {
    try {
      return this.state.storage.transactionSync((): ObjectReply => {
        const existing = userTables(sql);
        if (existing.length > 0) {
          throw new SeedRefused(`storage is not fresh: [${existing.join(", ")}]`);
        }
        for (const statement of statements)
          sql.exec(statement.sql, ...(statement.bindings as SqlValue[]));
        if (project === undefined) return { kind: "seeded" };
        const counts = publishSnapshot(sql, project);
        sql.exec(
          "INSERT INTO _fleet_sync_schema_steps(version, record) VALUES(?, ?)",
          project.schema.version,
          schemaStepJson(project),
        );
        sql.exec(
          "INSERT INTO _fleet_sync_copy(singleton, record) VALUES(1, ?)",
          copyRecordJson(project),
        );
        return { kind: "seeded", counts };
      });
    } catch (error) {
      if (error instanceof SeedRefused) return { kind: "refused", reason: error.message };
      throw error;
    }
  }
}

export class LeaseRegistryObject extends SeedStorageObject {}
export class ConnectionObject extends SeedStorageObject {}
export class ProjectCopyObject extends SeedStorageObject {}
export class MailboxObject extends SeedStorageObject {}
export class FleetObject extends SeedStorageObject {}
