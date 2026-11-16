import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/workspace-architecture-check",
  files: [
    {
      file: "crates/workspace-architecture-check/src/cycle_baseline.rs",
      disposition: { kind: "none" },
      note: "module-cycle ratchet check; serde/json for CycleBaseline, FrozenComponent, no outside-edge shape",
    },
    {
      file: "crates/workspace-architecture-check/src/cycle_baseline/tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared in cycle_baseline.rs:268-269 #[cfg(test)] mod tests",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/workspace-architecture-check/src/dependency_graph.rs",
      disposition: { kind: "none" },
      note: "module-cycle ratchet check; serde/json for ModuleEdge, no outside-edge shape",
    },
    {
      file: "crates/workspace-architecture-check/src/dependency_graph/tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared in dependency_graph.rs:209-210 #[cfg(test)] mod tests",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/workspace-architecture-check/src/error.rs",
      disposition: { kind: "none" },
      note: "module-cycle ratchet check",
    },
    {
      file: "crates/workspace-architecture-check/src/lib.rs",
      disposition: { kind: "none" },
      note: "module-cycle ratchet check",
    },
    {
      file: "crates/workspace-architecture-check/src/workspace_scan.rs",
      disposition: { kind: "none" },
      note: "module-cycle ratchet check; serde/json for WorkspaceMetadata, MetadataPackage, MetadataTarget, no outside-edge shape",
    },
    {
      file: "crates/workspace-architecture-check/src/workspace_scan/tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared in workspace_scan.rs:184-185 #[cfg(test)] mod tests",
      },
      note: "unit-test module, not product code",
    },
  ],
  sendSites: [],
};
