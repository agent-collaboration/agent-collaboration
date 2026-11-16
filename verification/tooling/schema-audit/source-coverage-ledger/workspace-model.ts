import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/workspace-model",
  files: [
    {
      file: "crates/workspace-model/src/central_mail.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central workspace contract (Router<->central link and central request handling), depended on only by central-link-client/-protocol, collaboration-central-core/-cloudflare/-tokio and workspace-rules (Cargo.toml); not one of the four protocols: CentralMailRequest l13, CentralMailOperation l25, CentralMailAnswer l61, CentralMailRefusal l109, MessageRecord l216, SessionQueue l230 (carried by central-link-protocol link_request.rs:3)",
      },
      note: "targets are central SessionAddress {machineId,endpoint,sessionId} (entity_records.rs:81), a central identity distinct from board SessionRef; no Router-side conversion between them found (rg SessionAddress: central crates only)",
    },
    {
      file: "crates/workspace-model/src/entity_records.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central workspace contract (Router<->central link and central request handling), depended on only by central-link-client/-protocol, collaboration-central-core/-cloudflare/-tokio and workspace-rules (Cargo.toml); not one of the four protocols: RoleName l50, AssignmentState l57, MembershipState l63, SessionAddress l81, Identity l103, Addressee l133, ChangeEvent l161",
      },
      note: "central types: RoleName, AssignmentState, MembershipState",
    },
    {
      file: "crates/workspace-model/src/item_references.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central workspace contract (Router<->central link and central request handling), depended on only by central-link-client/-protocol, collaboration-central-core/-cloudflare/-tokio and workspace-rules (Cargo.toml); not one of the four protocols: ItemKind l9, ItemRef l48, ReferenceSource l105, Reference l127 (details: Option<serde_json::Value> l131), ExternalLink l167",
      },
      note: "central types: ItemKind, ItemRef, ReferenceSource",
    },
    {
      file: "crates/workspace-model/src/lib.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central workspace contract (Router<->central link and central request handling), depended on only by central-link-client/-protocol, collaboration-central-core/-cloudflare/-tokio and workspace-rules (Cargo.toml); not one of the four protocols: module root and serialized-length validation helper (serde_json::to_string l19)",
      },
      note: "module root, re-exports of workspace-ids, encode_json helper l17-22",
    },
    {
      file: "crates/workspace-model/src/request_contract.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central workspace contract (Router<->central link and central request handling), depended on only by central-link-client/-protocol, collaboration-central-core/-cloudflare/-tokio and workspace-rules (Cargo.toml); not one of the four protocols: NoticeCadence l14, Caller l43, ActorRef l67, WorkspaceOperation l80, WorkspaceAnswer l254, WorkspaceReply l333, WorkspaceRequest l339 (carried by central-link-protocol link_request.rs:4)",
      },
      note: "central types: NoticeCadence, Caller, ActorRef",
    },
    {
      file: "crates/workspace-model/src/rule_commands.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central workspace contract (Router<->central link and central request handling), depended on only by central-link-client/-protocol, collaboration-central-core/-cloudflare/-tokio and workspace-rules (Cargo.toml); not one of the four protocols: OrganizationCommand l17, RoleCommand l60, ThreadCommand l89, BacklogCommand l131, WorkspaceCommand l281",
      },
      note: "central types: OrganizationCommand, RoleCommand, ThreadCommand",
    },
    {
      file: "crates/workspace-model/src/rule_results.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central workspace contract (Router<->central link and central request handling), depended on only by central-link-client/-protocol, collaboration-central-core/-cloudflare/-tokio and workspace-rules (Cargo.toml); not one of the four protocols: NamedItem l10, Blocker l16, DeliverableView l23, RefusalReason l47, ItemView l112, RuleAnswer l160, InboxEntry l178",
      },
      note: "central types: NamedItem, Blocker, DeliverableView",
    },
    {
      file: "crates/workspace-model/src/validated_values.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central workspace contract (Router<->central link and central request handling), depended on only by central-link-client/-protocol, collaboration-central-core/-cloudflare/-tokio and workspace-rules (Cargo.toml); not one of the four protocols: BacklogDescription l64, PageLimit l142, ChangePosition l173 validated values",
      },
      note: "central types: BacklogDescription, PageLimit, ChangePosition",
    },
  ],
  sendSites: [],
};
