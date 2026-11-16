import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/workspace-ids",
  files: [
    {
      file: "crates/workspace-ids/src/lib.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central workspace identifier newtypes WorkspaceId..AutomationId (macro l12-44, list l46-75; serde try_from String, UUIDv7 check l18-27), depended on only by central crates (central-link-protocol, collaboration-central-core/-cloudflare/-tokio, workspace-model Cargo.toml); the Router<->central link is not one of the four protocols",
      },
      note: "ported from message-board board_identity (comment l11) without minting; a central-side copy of the board id newtype mechanism, not a Router vocabulary type",
    },
  ],
  sendSites: [],
};
