// Test only. Stands in for production's old central: a Worker with the same
// name and the same five storage classes, so storage it writes is the storage
// the migration Worker then opens (Miniflare keeps it under one persistence
// path, as Cloudflare keeps it under one Worker name). It runs SQL it is given
// and dumps every table, so a test can compare storage before and after.

interface SqlStorage {
  exec(query: string, ...bindings: unknown[]): { toArray(): Record<string, unknown>[] };
}
interface State {
  readonly storage: { readonly sql: SqlStorage };
}

class OldCentralObject {
  private readonly state: State;
  constructor(state: State) {
    this.state = state;
  }
  async fetch(request: Request): Promise<Response> {
    const sql = this.state.storage.sql;
    const path = new URL(request.url).pathname;
    if (path === "/run") {
      const statements = (await request.json()) as string[];
      for (const statement of statements) sql.exec(statement);
      return Response.json({ ran: statements.length });
    }
    if (path === "/dump") {
      const tables = sql
        .exec(
          "SELECT name FROM sqlite_schema WHERE type IN ('table','index') AND name NOT LIKE '_cf_%' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .toArray()
        .map((row) => String(row.name));
      const schema = sql
        .exec(
          "SELECT type, name, tbl_name, sql FROM sqlite_schema WHERE name NOT LIKE '_cf_%' AND name NOT LIKE 'sqlite_%' ORDER BY type, name",
        )
        .toArray();
      const rows: Record<string, unknown[]> = {};
      for (const table of tables) {
        const kind = sql.exec("SELECT type FROM sqlite_schema WHERE name = ?", table).toArray();
        if (kind[0]?.type !== "table") continue;
        rows[table] = sql
          .exec(`SELECT * FROM "${table}" ORDER BY rowid`)
          .toArray()
          .map((row) =>
            Object.fromEntries(
              Object.entries(row).map(([column, value]) => [
                column,
                value instanceof ArrayBuffer
                  ? {
                      blob: Array.from(new Uint8Array(value), (byte) =>
                        byte.toString(16).padStart(2, "0"),
                      ).join(""),
                    }
                  : value,
              ]),
            ),
          );
      }
      return Response.json({ schema, rows });
    }
    return new Response("unknown", { status: 404 });
  }
}

export class LeaseRegistryObject extends OldCentralObject {}
export class ConnectionObject extends OldCentralObject {}
export class ProjectCopyObject extends OldCentralObject {}
export class MailboxObject extends OldCentralObject {}
export class FleetObject extends OldCentralObject {}

interface Namespace {
  idFromName(name: string): unknown;
  get(id: unknown): { fetch(request: Request): Promise<Response> };
}

export default {
  async fetch(request: Request, env: Record<string, Namespace>): Promise<Response> {
    // /<BINDING>/<object name>/<run|dump>
    const [, binding = "", name = "", action = ""] = new URL(request.url).pathname.split("/");
    const namespace = env[binding];
    if (namespace === undefined) return new Response("no binding", { status: 404 });
    const stub = namespace.get(namespace.idFromName(decodeURIComponent(name)));
    return stub.fetch(new Request(`https://object/${action}`, request));
  },
};
