import type { LedgerSection } from "../schema-audit-types.ts";

const R = "crates/board-sync-protocol/src/";

export const section: LedgerSection = {
  root: "crates/board-sync-protocol",
  files: [
    {
      file: `${R}board_delivery.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "Frozen local SDK custody payloads for message, notification and wake. BoardDeliveryTypes uses message_board::SessionRef for its address.",
      },
    },
    {
      file: `${R}lib.rs`,
      disposition: { kind: "none" },
      note: "module declarations and re-exports only",
    },
  ],
  sendSites: [],
};
