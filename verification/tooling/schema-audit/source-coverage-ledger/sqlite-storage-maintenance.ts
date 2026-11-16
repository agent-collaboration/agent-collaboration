import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/sqlite-storage-maintenance",
  files: [
    {
      file: "crates/sqlite-storage-maintenance/src/calendar_retention.rs",
      disposition: {
        kind: "none",
      },
      note: "calendar-month retention cutoff arithmetic; no serde, no protocol shapes",
    },
    {
      file: "crates/sqlite-storage-maintenance/src/commit_signal.rs",
      disposition: {
        kind: "none",
      },
      note: "CommitSignal l17 / SignaledTransaction l99 commit counter for Router SQLite stores; no serde, no protocol shapes",
    },
    {
      file: "crates/sqlite-storage-maintenance/src/failed_pass_backoff.rs",
      disposition: {
        kind: "none",
      },
      note: "FailedPassBackoff l15 retry timing 1 s..30 s; no protocol shapes",
    },
    {
      file: "crates/sqlite-storage-maintenance/src/lib.rs",
      disposition: {
        kind: "none",
      },
      note: "auto-vacuum preparation and WAL checkpoint (ExistingDatabasePreparation l30, WalCheckpointOutcome l70); no serde, no protocol shapes",
    },
    {
      file: "crates/sqlite-storage-maintenance/src/tests.rs",
      disposition: {
        kind: "excluded",
        reason: "cfg(test) module declared at lib.rs:422-423 (#[cfg(test)] mod tests)",
      },
      note: "unit tests with FixedAvailableSpaceProbe l15; no protocol shapes",
    },
  ],
  sendSites: [],
};
