import type { InventoryRow } from "../schema-audit-types.ts";

const R = "crates/fleet-identity-protocol/src/";

export const rows: InventoryRow[] = [
  {
    file: `${R}remote_start.rs`,
    line: 39,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "RemoteLaunchAccess {write-restricted, workspace-write} (kebab-case) in RemoteLaunchPolicy l34: a second copy of collaboration-protocol RouterAccess (access_contract.rs:7, also kebab-case at :6, so the two spell the same values) and ProviderRequestedPolicy, mapped by hand both ways at collaboration-protocol provider_conversation_contract.rs:249,260",
    usedBy: [{ crate: "fleet-identity-protocol" }, { crate: "collaboration-protocol" }],
    duplicateGroup: "router-access-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
];
