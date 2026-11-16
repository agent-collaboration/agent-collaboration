import type { LedgerSection } from "../schema-audit-types.ts";

const R = "crates/central-link-client/src/";

export const section: LedgerSection = {
  root: "crates/central-link-client",
  files: [
    {
      file: `${R}error.rs`,
      disposition: { kind: "none" },
      note: "ClientError l2 local error enum only",
    },
    {
      file: `${R}lib.rs`,
      disposition: { kind: "none" },
      note: "module declarations and re-exports; the Host owns socket I/O (l1)",
    },
    {
      file: `${R}link_state.rs`,
      disposition: { kind: "none" },
      note: "Router-central link state machine over central-link-protocol frames (begin_request l182 builds DataFrame::Request, receive l213 handles Welcome/Pong/Reply/Deliver); no socket I/O, no serde type of its own; the Router-central link is not one of the six outside edges",
    },
    {
      file: `${R}pending_calls.rs`,
      disposition: { kind: "none" },
      note: "in-flight CorrelationId bookkeeping and deadlines (PendingCalls l36, failed_call l140) over existing central-link-protocol types; no serde shape",
    },
  ],
  sendSites: [],
};
