import type { InventoryRow } from "../schema-audit-types.ts";

export const rows: InventoryRow[] = [
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
