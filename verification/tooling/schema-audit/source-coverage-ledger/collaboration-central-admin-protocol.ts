import type { LedgerSection } from "../schema-audit-types.ts";

const R = "crates/collaboration-central-admin-protocol/src/";
const admin =
  "central owner-only /admin/v1 HTTP API (lib.rs:1), not one of the six outside edges and not reachable from collaboration-mcp or collaboration-protocol";

export const section: LedgerSection = {
  root: "crates/collaboration-central-admin-protocol",
  files: [
    {
      file: `${R}copy_health.rs`,
      disposition: {
        kind: "excluded",
        reason: `${admin}: CopyStage l6, CopyHealthRead l14, CopyBookkeepingVersion l20, CopyMigrationFailureRecord l27, StagedPartHealth l35, LiveCopyHealth l42, CopyHealth l59`,
      },
    },
    {
      file: `${R}copy_operations.rs`,
      disposition: {
        kind: "excluded",
        reason: `${admin}: CopyDirectoryPage l7, CopyMigrationResult l20, CopyMigrationItem l40, CopyMigrationPage l47, CopyPageQuery l54, CopyRepairAnswer l60, OwnerOperationFailure l65`,
      },
    },
    {
      file: `${R}lib.rs`,
      disposition: { kind: "none" },
      note: "module declarations and fleet_sync_protocol re-exports only",
    },
    {
      file: `${R}registry_operations.rs`,
      disposition: {
        kind: "excluded",
        reason: `${admin}: OwnerMove l12, OwnerTakeover l19, OwnerRetirement l27, OwnerPreviewSuccess l34, OwnerTransferAnswer l47, OwnerBlockedSubject l74, OwnerRetirementPhase l79, OwnerRetirementAnswer l92, OwnerPreviewRefusal l111, OwnerPreviewResponse l119`,
      },
    },
  ],
  sendSites: [],
};
