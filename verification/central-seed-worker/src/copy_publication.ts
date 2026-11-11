// Publish a project's head snapshot into a fresh old-shape Copy, following the
// old receiver's seed publication (fleet-sync b9da4b5 seed_publication.rs):
// schema steps in order with replicated membership, every member table
// cleared, rows bound to each table's full column layout, per-table digests
// recomputed from what SQLite stored and checked against the snapshot
// manifest, foreign keys validated, then the schema step and the Following
// record. Synchronous, so it runs inside one storage transaction: any refusal
// rolls the whole Copy back.

import type { SqlStorage, SqlValue } from "./platform.ts";
import { refuse, transportToSeed, type SeedProject } from "./seed_contract.ts";
import { serdeValue, tableDigest, type SeedValue } from "./table_digest.ts";

/** fleet-sync-protocol reserved_namespace.rs: never replicated. */
export function reservedNamespace(name: string): boolean {
  const lower = name.toLowerCase();
  return (
    lower === "_sqlx_migrations" ||
    ["sqlite_", "turso_", "__turso_internal", "_cf_", "__miniflare_", "_fleet_sync_"].some(
      (prefix) => lower.startsWith(prefix),
    )
  );
}

const quoted = (name: string): string => `"${name.replaceAll('"', '""')}"`;
const IDENTIFIER = String.raw`(?:"((?:[^"]|"")+)"|\[([^\]]+)\]|\x60([^\x60]+)\x60|([A-Za-z_][A-Za-z0-9_$]*))`;
const unquote = (match: RegExpExecArray, offset: number): string =>
  (match[offset] ?? match[offset + 1] ?? match[offset + 2] ?? match[offset + 3] ?? "")
    .replaceAll('""', '"')
    .toLowerCase();

/** The membership change one replicated schema statement makes (schema_membership.rs apply). */
function membershipChange(sql: string): { readonly add?: string; readonly remove?: string } {
  const text = sql.trim();
  const create = new RegExp(
    String.raw`^CREATE\s+(?:TEMP\w*\s+)?(TABLE|VIEW)\s+(?:IF\s+NOT\s+EXISTS\s+)?(?:main\.)?${IDENTIFIER}`,
    "i",
  ).exec(text);
  if (create !== null) {
    if (/^CREATE\s+TEMP/i.test(text)) refuse("schema step", "temporary schema cannot replicate");
    return { add: unquote(create, 2) };
  }
  const drop = new RegExp(
    String.raw`^DROP\s+(TABLE|VIEW)\s+(?:IF\s+EXISTS\s+)?(?:main\.)?${IDENTIFIER}`,
    "i",
  ).exec(text);
  if (drop !== null) return { remove: unquote(drop, 2) };
  const rename = new RegExp(
    String.raw`^ALTER\s+TABLE\s+(?:main\.)?${IDENTIFIER}\s+RENAME\s+TO\s+${IDENTIFIER}`,
    "i",
  ).exec(text);
  if (rename !== null) return { remove: unquote(rename, 1), add: unquote(rename, 5) };
  return {};
}

interface Column {
  readonly name: string;
  readonly keyOrder: number;
}
function columns(sql: SqlStorage, table: string): Column[] {
  return sql
    .exec<{ name: string; pk: number }>(
      `SELECT name, pk FROM pragma_table_info(?) ORDER BY cid`,
      table,
    )
    .toArray()
    .map((row) => ({ name: row.name, keyOrder: Number(row.pk) }));
}
const keyColumns = (layout: readonly Column[]): Column[] =>
  layout.filter((column) => column.keyOrder > 0).sort((a, b) => a.keyOrder - b.keyOrder);

/** A value bound with its SQLite type kept: an integer from its exact decimal text. */
function binding(value: SeedValue): { readonly placeholder: string; readonly value: SqlValue } {
  switch (value.kind) {
    case "null":
      return { placeholder: "?", value: null };
    case "integer":
      return { placeholder: "CAST(? AS INTEGER)", value: value.value };
    case "real":
      return { placeholder: "?", value: value.value };
    case "text":
      return { placeholder: "?", value: value.value };
    case "blob":
      return {
        placeholder: "?",
        value: value.value.buffer.slice(
          value.value.byteOffset,
          value.value.byteOffset + value.value.byteLength,
        ) as ArrayBuffer,
      };
  }
}

/** Reads a table back as SqlValues, integers exactly through their text, in key order. */
function storedRows(sql: SqlStorage, table: string, layout: readonly Column[]): SeedValue[][] {
  const projection = layout
    .map(
      (column, index) =>
        `typeof(${quoted(column.name)}) AS t${index}, CAST(${quoted(column.name)} AS TEXT) AS x${index}, ${quoted(column.name)} AS v${index}`,
    )
    .join(", ");
  const order = keyColumns(layout)
    .map((column) => quoted(column.name))
    .join(", ");
  return sql
    .exec(`SELECT ${projection} FROM ${quoted(table)} ORDER BY ${order}`)
    .toArray()
    .map((row) =>
      layout.map((_, index): SeedValue => {
        const type = row[`t${index}`];
        const value = row[`v${index}`];
        switch (type) {
          case "null":
            return { kind: "null" };
          case "integer":
            return { kind: "integer", value: String(row[`x${index}`]) };
          case "real":
            return { kind: "real", value: Number(value) };
          case "text":
            return { kind: "text", value: String(value) };
          case "blob":
            return { kind: "blob", value: new Uint8Array(value as ArrayBuffer) };
          default:
            return refuse(`${table}`, `unexpected storage class ${String(type)}`);
        }
      }),
    );
}

function validateForeignKeys(sql: SqlStorage, tables: readonly string[]): void {
  for (const table of tables) {
    const rows = sql
      .exec<{
        id: number;
        seq: number;
        table: string;
        from: string;
        to: string | null;
        on_update: string;
        on_delete: string;
      }>(
        `SELECT id, seq, "table", "from", "to", on_update, on_delete FROM pragma_foreign_key_list(?) ORDER BY id, seq`,
        table,
      )
      .toArray();
    const groups = new Map<number, { parent: string; pairs: [string, string | null][] }>();
    for (const row of rows) {
      if (
        ![row.on_update, row.on_delete].every(
          (action) => action === "NO ACTION" || action === "RESTRICT",
        )
      ) {
        refuse(table, "foreign key action other than NO ACTION/RESTRICT");
      }
      const group = groups.get(row.id) ?? { parent: row.table, pairs: [] };
      group.pairs.push([row.from, row.to === null || row.to === "" ? null : row.to]);
      groups.set(row.id, group);
    }
    for (const { parent, pairs } of groups.values()) {
      if (!tables.some((name) => name === parent.toLowerCase()))
        refuse(table, `references non-replicated ${parent}`);
      let resolved = pairs;
      if (pairs.every(([, to]) => to === null)) {
        const keys = keyColumns(columns(sql, parent));
        if (keys.length !== pairs.length)
          refuse(table, `foreign key to ${parent} doesn't match its key`);
        resolved = pairs.map(([from], index) => [from, keys[index]?.name ?? null]);
      }
      const notNull = resolved.map(([from]) => `child.${quoted(from)} IS NOT NULL`).join(" AND ");
      const matching = resolved
        .map(([from, to]) => `parent.${quoted(to ?? "")} = +child.${quoted(from)}`)
        .join(" AND ");
      const [violation] = sql
        .exec<{ count: number }>(
          `SELECT count(*) AS count FROM ${quoted(table)} AS child WHERE ${notNull} AND NOT EXISTS (SELECT 1 FROM ${quoted(parent)} AS parent WHERE ${matching})`,
        )
        .toArray();
      if ((violation?.count ?? 0) > 0) refuse(table, `rows reference missing ${parent} rows`);
    }
  }
}

const UUID_V7 = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

/**
 * Applies the snapshot inside the caller's transaction and returns the
 * published tables' row counts. Throws SeedRefused for any mismatch.
 */
export function publishSnapshot(sql: SqlStorage, project: SeedProject): Record<string, number> {
  sql.exec("PRAGMA defer_foreign_keys=ON");
  const members = new Set<string>();
  for (const step of project.schemaSteps) {
    const change = membershipChange(step);
    if (change.add !== undefined && reservedNamespace(change.add))
      refuse("schema step", `reserved name ${change.add}`);
    sql.exec(step);
    if (change.remove !== undefined) {
      members.delete(change.remove);
      sql.exec("DELETE FROM _fleet_sync_copy_tables WHERE name = ?", change.remove);
    }
    if (change.add !== undefined) {
      members.add(change.add);
      sql.exec(
        "INSERT INTO _fleet_sync_copy_tables(name) VALUES(?) ON CONFLICT(name) DO NOTHING",
        change.add,
      );
    }
  }
  const tables = sql
    .exec<{ name: string }>("SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name")
    .toArray()
    .map((row) => row.name)
    .filter((name) => !reservedNamespace(name) && members.has(name.toLowerCase()))
    .sort();
  const offered = project.manifest.map((entry) => entry.table).sort();
  if (offered.join("\n") !== tables.join("\n")) {
    refuse(
      "manifest",
      `tables [${offered.join(", ")}] differ from the schema's [${tables.join(", ")}]`,
    );
  }
  // Data migrations in the schema steps can create rows; the snapshot replaces them.
  for (const table of tables) sql.exec(`DELETE FROM ${quoted(table)}`);
  for (const page of project.pages) {
    if (page.table === "" && page.rows.length === 0) continue;
    if (!tables.includes(page.table))
      refuse(`page ${page.table}`, "table is outside the replicated schema");
    const layout = columns(sql, page.table);
    const keys = new Set(keyColumns(layout).map((column) => column.name));
    page.rows.forEach((row, rowIndex) => {
      const where = `${page.table}[${rowIndex}]`;
      if (row.length !== layout.length) refuse(where, "row width differs from the table's columns");
      const values = row.map((value, index) => transportToSeed(value, `${where}[${index}]`));
      layout.forEach((column, index) => {
        const value = values[index];
        if (keys.has(column.name) && !(value?.kind === "text" && UUID_V7.test(value.value))) {
          refuse(where, `key ${column.name} is not a UUIDv7 text value`);
        }
      });
      const bound = values.map(binding);
      sql.exec(
        `INSERT INTO ${quoted(page.table)}(${layout.map((column) => quoted(column.name)).join(", ")}) VALUES(${bound.map((entry) => entry.placeholder).join(", ")})`,
        ...bound.map((entry) => entry.value),
      );
    });
  }
  const counts: Record<string, number> = {};
  for (const table of tables) {
    const layout = columns(sql, table);
    const rows = storedRows(sql, table, layout);
    const entry = project.manifest.find((candidate) => candidate.table === table);
    if (entry === undefined || BigInt(entry.rowCount) !== BigInt(rows.length)) {
      refuse(
        `table ${table}`,
        `stored ${rows.length} rows, manifest says ${String(entry?.rowCount)}`,
      );
    }
    const digest = tableDigest(rows);
    if (digest !== entry?.digest) {
      // Name the first stored value whose serde text differs from what was sent.
      const sent = project.pages
        .filter((page) => page.table === table)
        .flatMap((page) => page.rows)
        .map((row, index) =>
          row.map((value, column) =>
            serdeValue(transportToSeed(value, `${table}[${index}][${column}]`)),
          ),
        );
      const stored = rows.map((row) => row.map(serdeValue));
      const changed = stored.flatMap((row, index) =>
        row.flatMap((text, column) =>
          text === sent[index]?.[column]
            ? []
            : [`row ${index} column ${column}: sent ${sent[index]?.[column]}, stored ${text}`],
        ),
      )[0];
      refuse(
        `table ${table}`,
        `digest ${digest} differs from the manifest's ${String(entry?.digest)}${changed === undefined ? "" : ` (${changed})`}`,
      );
    }
    counts[table] = rows.length;
  }
  validateForeignKeys(sql, tables);
  sql.exec("PRAGMA defer_foreign_keys=OFF");
  return counts;
}

/** JSON text with u64 fields written from their exact decimals. */
export function copyRecordJson(project: SeedProject): string {
  const position = (value: { epoch: string; sequence: string }) =>
    `{"epoch":${value.epoch},"sequence":${value.sequence}}`;
  return (
    `{"state":"following","copy":{"database":${JSON.stringify(project.database)},"role":"authoritative",` +
    `"fence":${project.leaseNumber},"holder":${JSON.stringify(project.holder)},` +
    `"start":${position(project.start)},"acked":${position(project.head)},` +
    `"schema":{"version":${project.schema.version},"fingerprint":${JSON.stringify(project.schema.fingerprint)}}}}`
  );
}

export function schemaStepJson(project: SeedProject): string {
  return `{"version":${project.schema.version},"statements":${JSON.stringify(project.schemaSteps)}}`;
}
