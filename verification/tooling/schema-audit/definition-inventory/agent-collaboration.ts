import type { InventoryRow } from "../schema-audit-types.ts";

export const rows: InventoryRow[] = [
  {
    file: "crates/agent-collaboration/src/approval_commands.rs",
    line: 128,
    owner: "router-vocabulary",
    kind: "approval-table",
    defines:
      "--allow / --allow-for-session / --deny flags to ApprovalDecision Allow, AllowForSession, Deny (L128-L136); --option-id passes an option id with no decision",
    usedBy: [{ crate: "agent-collaboration" }],
    duplicateGroup: "approval-option-tables",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/agent-collaboration/src/approval_commands.rs",
    line: 219,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "approval error envelope {kind:error,error:OperationFailure} built with json! (L219, L237)",
    usedBy: [{ crate: "agent-collaboration" }],
    duplicateGroup: "cli-error-envelopes",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/agent-collaboration/src/approval_commands.rs",
    line: 249,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "parse_actor: --actor JSON decoded with the canonical Serde as Identity when it has a kind field, else as a SessionRef wrapped in Identity::Session (L249-L256); typed decode, no hand-written SessionRef shape",
    usedBy: [{ crate: "agent-collaboration" }],
    duplicateGroup: "session-ref-conversion",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/agent-collaboration/src/automation_argument_feedback.rs",
    line: 18,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "invalidField argument error envelope {kind:error,operationId:null,error:{kind,stage,message,field,constraint,effects,nextAction}} built with json! (L18-L28)",
    usedBy: [{ crate: "agent-collaboration" }],
    duplicateGroup: "cli-error-envelopes",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/agent-collaboration/src/claude_terminal_test_harness.rs",
    line: 39,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "stand-in endpoint inventory reply {serviceEpoch,sequence,endpoints:[{endpoint,label,channels,availability}]} built with json! and decoded by serde_json::from_value (L39-L45), feature quota-reset-test-harness",
    usedBy: [{ crate: "agent-collaboration" }],
    duplicateGroup: "router-vocabulary-stand-ins",
    disposition: "duplicate",
    fixedBy: "D2",
  },
];
