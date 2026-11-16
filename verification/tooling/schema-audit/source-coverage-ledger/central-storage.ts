import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/central-storage",
  files: [
    {
      file: "crates/central-storage/src/lib.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central core's private storage vocabulary (module doc l1-7), carried only between central core and its store host, not one of the six outside edges: Statement l13, SqlValue l23, SafeInteger l34, RowExpectation l76, GuardedStatement l94, StatementBatch l101, AlarmUpdate l109, BatchOutcome l121, ConstraintKind l134, ReadOutcome l143, StoreFailure l150, CorruptReason l161, StoreCall l170, StoreReply l177",
      },
    },
  ],
  sendSites: [],
};
