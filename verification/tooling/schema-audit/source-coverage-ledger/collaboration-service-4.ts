import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/collaboration-service",
  files: [
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/owner_setup.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/owner_fixture.rs:94 (#[path] mod), owner_fixture declared at crates/collaboration-service/src/subscription_delivery/tests.rs:13 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "test fixture builds central board requests (workspace/board/project create) as json! then decodes them (l70-l100); central board protocol, not an outside edge",
    },
    {
      file: "crates/collaboration-service/src/wakeup_projection.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "projects agent_automation WakeRecord into the MCP wake_send/wake_show/wake_pause/resume/cancel output WakeSnapshot / WakeMutationResult (collaboration-mcp catalog_tools.rs:118-124): FireReceipt copy l17, WakeState enum mapping l26, latest_event_cursor JSON-in-string l34, TimingRule->TimingRequest serde round trip l48, retained DeliveryRecord<Value,..>->DeliveryRecord<SessionRef,..> serde round trip l74",
    },
    {
      file: "crates/collaboration-service/src/wakeup_subscription.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "backs MCP wake_wait_until_first_fire (collaboration-mcp wait_tools.rs:54) via wake_operations.rs:462: parses the JSON-tuple cursor string l79 and rebuilds it l113, l185; maps stored transition kind strings to protocol WakeChange l149 and decodes body.fire into FirstFire l151, copied into FireReceipt l163",
    },
  ],
  sendSites: [],
};
