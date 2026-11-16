import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/sqlx-durable-object-core",
  files: [
    {
      file: "crates/sqlx-durable-object-core/src/arguments.rs",
      disposition: {
        kind: "none",
      },
      note: "SQLx argument binding into core Value (add_argument l16); SQL driver plumbing, no serde shapes",
    },
    {
      file: "crates/sqlx-durable-object-core/src/error.rs",
      disposition: {
        kind: "none",
      },
      note: "DatabaseError l16 and SQLite message-to-ErrorKind mapping (code l66, kind l88); SQL driver errors, no protocol shapes",
    },
    {
      file: "crates/sqlx-durable-object-core/src/js.rs",
      disposition: {
        kind: "none",
      },
      note: "wasm-bindgen conversion of SQL Values to/from JsValue (to_js l28, from_js l54); Durable Object SQL binding, no protocol shapes",
    },
    {
      file: "crates/sqlx-durable-object-core/src/lib.rs",
      disposition: {
        kind: "none",
      },
      note: "module declarations and re-exports (l23-40) only",
    },
    {
      file: "crates/sqlx-durable-object-core/src/result.rs",
      disposition: {
        kind: "none",
      },
      note: "QueryResult l3 / BatchResult l57 row counts; SQL driver plumbing, no serde",
    },
    {
      file: "crates/sqlx-durable-object-core/src/row.rs",
      disposition: {
        kind: "none",
      },
      note: "rows() l17 decodes SQL cursor rows into core Values; no protocol shapes",
    },
    {
      file: "crates/sqlx-durable-object-core/src/types.rs",
      disposition: {
        kind: "none",
      },
      note: "SQLx Type/Encode/Decode impls for core Value; no serde, no protocol shapes",
    },
    {
      file: "crates/sqlx-durable-object-core/src/value.rs",
      disposition: {
        kind: "none",
      },
      note: "SQL TypeInfo l16 / Value l67 and safe_integer l95; SQL value model, no serde",
    },
  ],
  sendSites: [],
};
