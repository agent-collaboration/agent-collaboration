import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/collaboration-mail-protocol",
  files: [
    {
      file: "crates/collaboration-mail-protocol/src/lib.rs",
      disposition: {
        kind: "none",
      },
      note: "module declarations and re-exports (l1-26) only",
    },
    {
      file: "crates/collaboration-mail-protocol/src/session_display_name.rs",
      disposition: {
        kind: "defines",
        owners: ["router-vocabulary"],
      },
      note: "SessionDisplayName validated sender label",
    },
  ],
  sendSites: [],
};
