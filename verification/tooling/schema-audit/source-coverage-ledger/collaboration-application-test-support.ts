import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/collaboration-application-test-support",
  files: [
    {
      file: "crates/collaboration-application-test-support/src/accepted_snapshot.rs",
      disposition: {
        kind: "none",
      },
      note: "central fleet-sync snapshot baseline (NoCalls CallRouter l14 returns serde_json::Value from durable_steps); central replication, not one of the four protocols",
    },
    {
      file: "crates/collaboration-application-test-support/src/board_context_backend.rs",
      disposition: {
        kind: "converts",
        owners: ["codex"],
      },
      note: "test stand-in Codex native endpoint: hand-built schema bundle {definitions:{v2:...}} l45-51, endpoint inventory l57; parses each request from its accepted WebSocket l93 and writes the initialize/thread/read reply {id,result} back over it l104-110 (apparatus send site)",
    },
    {
      file: "crates/collaboration-application-test-support/src/board_poll_services.rs",
      disposition: {
        kind: "none",
      },
      note: "FixturePollServices l6 test poll services; no serde",
    },
    {
      file: "crates/collaboration-application-test-support/src/lib.rs",
      disposition: {
        kind: "none",
      },
      note: "module declarations (l3-9) only",
    },
    {
      file: "crates/collaboration-application-test-support/src/native_mail_owner.rs",
      disposition: {
        kind: "none",
      },
      note: "native mail owner test double; no serde, no protocol shapes",
    },
    {
      file: "crates/collaboration-application-test-support/src/registered_project.rs",
      disposition: {
        kind: "none",
      },
      note: "registered project fixture helpers; no serde",
    },
  ],
  sendSites: [],
};
