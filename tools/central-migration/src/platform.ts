// The slice of the Workers runtime this Worker uses, typed locally so the
// one-time tool carries no extra type package. Shapes follow the Durable Object
// SQL storage API (ctx.storage.sql.exec returning an iterable cursor).

export type SqlValue = string | number | null | ArrayBuffer;
export type SqlRow = Record<string, SqlValue>;

export interface SqlCursor<Row extends SqlRow = SqlRow> extends Iterable<Row> {
  toArray(): Row[];
  readonly rowsWritten?: number;
}

export interface SqlStorage {
  exec<Row extends SqlRow = SqlRow>(query: string, ...bindings: SqlValue[]): SqlCursor<Row>;
}

export interface DurableObjectState {
  readonly storage: {
    readonly sql: SqlStorage;
    transactionSync<T>(body: () => T): T;
  };
}

export interface DurableObjectId {
  toString(): string;
}

export interface DurableObjectStub {
  fetch(input: string, init?: RequestInit): Promise<Response>;
}

export interface DurableObjectNamespace {
  idFromName(name: string): DurableObjectId;
  get(id: DurableObjectId): DurableObjectStub;
}
