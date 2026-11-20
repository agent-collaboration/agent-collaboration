import type { InventoryRow } from "../schema-audit-types.ts";

export const rows: InventoryRow[] = [
  {
    file: "crates/collaboration-mail-protocol/src/session_display_name.rs",
    line: 8,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "SessionDisplayName: validated sender label (1-120 chars, no controls or header arrows) with a hand JsonSchema; re-exported by collaboration-protocol message_content.rs:44",
    usedBy: [{ crate: "collaboration-protocol" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
];
