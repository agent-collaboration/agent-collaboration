import type { LedgerSection } from "../schema-audit-types.ts";

const R = "crates/fleet-admission/src/";
const centralFleet =
  "central fleet admission contract between Router and central, not one of the six outside edges; no fleet-admission type is reachable from collaboration-mcp or collaboration-protocol (no use there)";

export const section: LedgerSection = {
  root: "crates/fleet-admission",
  files: [
    {
      file: `${R}archive_report.rs`,
      disposition: {
        kind: "excluded",
        reason: `${centralFleet}: ArchiveReport l9, ArchiveReportOutcome l24, ArchiveReportRefusal l37`,
      },
      note: "serde types above plus local ArchiveFinalization l45, ArchiveReportDecision l50 and decide_archive_report l55",
    },
    {
      file: `${R}conformance_cases.rs`,
      disposition: { kind: "none" },
      note: 'cfg(feature = "test-support") module (lib.rs:34): non-serde FleetCase tables building FleetRequest and ArchiveReport fixtures; no message shape of the four protocols',
    },
    {
      file: `${R}directory_query.rs`,
      disposition: {
        kind: "excluded",
        reason: `${centralFleet}: DirectoryQuery l11, DirectorySide l25, CopyMetadataAsOf l32, CopyMetadata l39, LeaseMetadata l46, DirectoryLease l59, DirectoryLeaseState l74, DirectoryEntry l81`,
      },
      note: "plus DirectoryQuery::matches l92 filter logic",
    },
    {
      file: `${R}fleet_adr_rules.rs`,
      disposition: { kind: "none" },
      note: "adr_plan l22: ADR create/transition decision over message_board Adr records; no serde shape",
    },
    {
      file: `${R}fleet_decision.rs`,
      disposition: {
        kind: "excluded",
        reason: `${centralFleet}: FleetWrite l30, FleetFeedEntry l55, FleetRequestResult l74`,
      },
      note: "plus FleetAdmissionError l10, non-serde FleetDecision l61 and RecordEnvelope builder envelope l80",
    },
    {
      file: `${R}fleet_import_contract.rs`,
      disposition: {
        kind: "excluded",
        reason: `${centralFleet}: FleetImportDigest l11, ImportProvenance l35, ImportedBoardState l42, FleetImportRecord l54, FleetImportBundle l93, ImportedFleetRecord l113, ExistingFleetImport l125, FleetImportRefusal l137, FleetImportConflict l145, FleetImportCounts l152, FleetImportResult l160`,
      },
      note: "digest_content l24 hashes serde_json::to_vec of import records (diagnostic digest, not a type round trip)",
    },
    {
      file: `${R}fleet_import_rules.rs`,
      disposition: { kind: "none" },
      note: "reads_for_import l14 and decide_import l40 build message_board records from central import records (ImportedBoardState->BoardState l268); central fleet import, no shape of the four protocols",
    },
    {
      file: `${R}fleet_metadata_rules.rs`,
      disposition: { kind: "none" },
      note: "workspace_plan l12 and metadata_plan l106 board/workspace/directory decisions over message_board records; no serde shape",
    },
    {
      file: `${R}fleet_reads.rs`,
      disposition: {
        kind: "excluded",
        reason: `${centralFleet}: FleetNameScope l16, FleetReadKey l29, ProjectCreationDecision l79`,
      },
      note: "plus non-serde FleetReads l65, FleetNameOwner l70, FleetSnapshot l87 and reads_for l131",
    },
    {
      file: `${R}fleet_request.rs`,
      disposition: {
        kind: "excluded",
        reason: `${centralFleet}: FleetRequest l11, FleetOperation l27`,
      },
      note: "canonical_hash l85 hashes serde_json::to_vec of the request tuple (central request identity, not a type round trip)",
    },
    {
      file: `${R}fleet_rules.rs`,
      disposition: { kind: "none" },
      note: "decide l35 and finish l102: ledger, receipt and feed decisions over message_board records; no serde shape",
    },
    {
      file: `${R}lib.rs`,
      disposition: { kind: "none" },
      note: "module declarations and re-exports; cfg(test) modules l37-44",
    },
  ],
  sendSites: [],
};
