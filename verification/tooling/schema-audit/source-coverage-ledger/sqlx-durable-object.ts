import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/sqlx-durable-object",
  files: [
    {
      file: "crates/sqlx-durable-object/src/arguments.rs",
      disposition: {
        kind: "none",
      },
      note: "DoArguments impl over DoArgumentValue; SQLx driver for Durable Object storage, no serde",
    },
    {
      file: "crates/sqlx-durable-object/src/batch.rs",
      disposition: {
        kind: "none",
      },
      note: "DoConnection batch execution (l12); SQL driver plumbing, no protocol shapes",
    },
    {
      file: "crates/sqlx-durable-object/src/connection.rs",
      disposition: {
        kind: "none",
      },
      note: "DoConnection from worker::Storage (l44) and DoConnectOptions parsing (l56-65); SQL driver, no protocol shapes",
    },
    {
      file: "crates/sqlx-durable-object/src/database.rs",
      disposition: {
        kind: "none",
      },
      note: "SQLx Database impl types DoArguments l37, DoColumn l43, DoConnection l60, DoStatement l90; no serde, no protocol shapes",
    },
    {
      file: "crates/sqlx-durable-object/src/error.rs",
      disposition: {
        kind: "none",
      },
      note: "unsupported() SQLx error helper only",
    },
    {
      file: "crates/sqlx-durable-object/src/executor.rs",
      disposition: {
        kind: "none",
      },
      note: "SQLx Executor impls for DoConnection/DoTransaction (l190-191); no protocol shapes",
    },
    {
      file: "crates/sqlx-durable-object/src/js.rs",
      disposition: {
        kind: "none",
      },
      note: "SqlStorage exec binding via wasm-bindgen (exec l136); no protocol shapes",
    },
    {
      file: "crates/sqlx-durable-object/src/lib.rs",
      disposition: {
        kind: "none",
      },
      note: "module declarations and DoExecutor trait (l104); no protocol shapes",
    },
    {
      file: "crates/sqlx-durable-object/src/migrate.rs",
      disposition: {
        kind: "none",
      },
      note: "SQLx Migrate over a DO transaction (TransactionMigrate l223; MigrationRunError l47, HistoryFault l73); SQL migration history, no protocol shapes",
    },
    {
      file: "crates/sqlx-durable-object/src/row.rs",
      disposition: {
        kind: "none",
      },
      note: "DoRow column access (l9-32); no protocol shapes",
    },
    {
      file: "crates/sqlx-durable-object/src/transaction.rs",
      disposition: {
        kind: "none",
      },
      note: "DoConnection transaction manager (l11); no protocol shapes",
    },
    {
      file: "crates/sqlx-durable-object/src/types.rs",
      disposition: {
        kind: "none",
      },
      note: "SQLx Type impls for Do; no serde, no protocol shapes",
    },
    {
      file: "crates/sqlx-durable-object/src/value.rs",
      disposition: {
        kind: "none",
      },
      note: "AsValue for DoValueRef (l4); no protocol shapes",
    },
  ],
  sendSites: [],
};
