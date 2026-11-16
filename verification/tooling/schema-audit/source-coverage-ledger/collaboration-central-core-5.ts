import type { LedgerSection } from "../schema-audit-types.ts";

const workspaceReadSqlReason =
  "central workspace read query, not one of the four protocols: hand-builds workspace_model row JSON with json_object per row_kind (decoded by WorkspaceRows::decode, workspace_row_codec.rs:60); workspace-model is used by no Router crate, so these rows never reach MCP/ACP/Codex";

export const section: LedgerSection = {
  root: "crates/collaboration-central-core",
  files: [
    {
      file: "crates/collaboration-central-core/src/session_scope/fleet_reads.rs",
      disposition: { kind: "none" },
      note: "stage_fleet_read (:14) maps board_sync BoardReadScope/Cursor to FleetReadRequest and decodes DirectoryQuery from BoardPayload (:24); fleet-sync reads, not the four protocols",
    },
    {
      file: "crates/collaboration-central-core/src/session_scope/forwards.rs",
      disposition: { kind: "none" },
      note: "forwarded-post routing (stage_forward :30, finish_forward :126) serializes fleet ServerFrame Answer with ForwardOutcome (:135-138) onto the fleet socket; fleet-sync only",
    },
    {
      file: "crates/collaboration-central-core/src/session_scope/operations.rs",
      disposition: { kind: "none" },
      note: "stage_operation (:26) routes fleet OperationRequest; builds message_board::RequestEffectsRequest {scope: Fleet} from BoardReadScope::RequestEffects (:245-251) for a central fleet-effects read, not an MCP request; stage_operation_answer (:286) serializes ServerFrame onto the fleet socket",
    },
    {
      file: "crates/collaboration-central-core/src/session_store.rs",
      disposition: { kind: "none" },
      note: "PendingWelcome/SessionTransportEffect/SessionCommit (:28, :34, :128) in-memory effect queue; no serde",
    },
    {
      file: "crates/collaboration-central-core/src/shared_task_registry.rs",
      disposition: { kind: "none" },
      note: "registry_engine (:15) registers fleet_sync_lease durable tasks; no serde",
    },
    {
      file: "crates/collaboration-central-core/src/sign_in_trust_settings.rs",
      disposition: { kind: "none" },
      note: "SignInTrustSettings (:6) plain config struct; no serde",
    },
    {
      file: "crates/collaboration-central-core/src/signature_verification.rs",
      disposition: { kind: "none" },
      note: "SignatureValidity/VerifyFailure/Ed25519Verification (:5, :10, :15); no serde",
    },
    {
      file: "crates/collaboration-central-core/src/signin_observation_empty.json",
      disposition: {
        kind: "excluded",
        reason:
          'test-support fixture: EMPTY_SIGNIN_OBSERVATION_JSON include_str! at signin_test_observation.rs:26 (module compiled only under cfg(feature = "test-support") at lib.rs:6); empty SignInObservation, central sign-in conformance only',
      },
      note: "empty sign-in observation fixture",
    },
    {
      file: "crates/collaboration-central-core/src/signin_test_observation.rs",
      disposition: {
        kind: "excluded",
        reason:
          'test-support observation, compiled only under cfg(feature = "test-support") declared at lib.rs:6: SignInObservation, ObservedStamp, ObservedOrganizationFloor, ObservedPersonFloor, ObservedUsedRequest, ObservedWorkspace, ObservedChangeRecord (signin_test_observation.rs:32-89) and SQL-built SnapshotRow JSON (116-179) observe central organization storage, not one of the four protocols',
      },
      note: "stamp_facts (:277) reads JWS header/claims via serde_json::Value",
    },
    {
      file: "crates/collaboration-central-core/src/signin_test_observation/machines.rs",
      disposition: {
        kind: "excluded",
        reason:
          'test-support observation (parent signin_test_observation compiled under cfg(feature = "test-support") at lib.rs:6, child declared at signin_test_observation.rs:19): ObservedMachineState, ObservedMachine, ObservedApprovalState, ObservedApprovalEndReason, ObservedMachineApproval (machines.rs:20-68) observe central machine enrollment rows',
      },
      note: "StoredMachineRow untagged scalar map (:83) back to ReadOutcome",
    },
    {
      file: "crates/collaboration-central-core/src/step_conformance.rs",
      disposition: {
        kind: "excluded",
        reason:
          'conformance-only step controls, compiled only under cfg(feature = "conformance") declared at lib.rs:430: StepControl, StepControlReply (step_conformance.rs:195, 217) drive central Fleet decision steps over message_board RequestLedgerEntry/DecisionEffect, not one of the four protocols',
      },
      note: "central conformance control surface",
    },
    {
      file: "crates/collaboration-central-core/src/step_conformance/claim_reservation.rs",
      disposition: {
        kind: "excluded",
        reason:
          'conformance-only (parent step_conformance under cfg(feature = "conformance") at lib.rs:430, child declared at step_conformance.rs:13): ReadClaimReservation step (claim_reservation.rs:15), no serde',
      },
      note: "conformance step",
    },
    {
      file: "crates/collaboration-central-core/src/step_conformance/native_callback.rs",
      disposition: {
        kind: "excluded",
        reason:
          'conformance-only (parent step_conformance under cfg(feature = "conformance") at lib.rs:430, child declared cfg(not(target_arch = "wasm32")) at step_conformance.rs:14-16): AfterDecisionWrite step (native_callback.rs:14), no serde',
      },
      note: "conformance step",
    },
    {
      file: "crates/collaboration-central-core/src/step_driver.rs",
      disposition: { kind: "none" },
      note: "StepDriver/StepCallback (:21, :72) transaction runner; records MigrationFailure on rollback (:51-62); no serde shapes",
    },
    {
      file: "crates/collaboration-central-core/src/storage_conformance.rs",
      disposition: {
        kind: "excluded",
        reason:
          'test-support storage conformance fixture, compiled only under cfg(feature = "test-support") declared at lib.rs:8: widgets migrator and statements (storage_conformance.rs:29-158) over central_storage, no message shapes',
      },
      note: "SQL fixture statements only",
    },
    {
      file: "crates/collaboration-central-core/src/wire_contract.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object wire DTO, not one of the four protocols: WireCall, WireResult (wire_contract.rs:11, 22) wrap RegistryCall/FleetCall for central object transport (decoded at object_transport.rs:192)",
      },
      note: "central object call union",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_actor_identity.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central workspace actor identity storage, not one of the four protocols: identity_fields canonical-JSON encodes workspace_model Identity (incl. AgentSession { session: SessionAddress }) into the actor_identities.identity column (workspace_actor_identity.rs:16-23) and decode_actor reads it back (165); same type both ways, workspace_model SessionAddress is not converted to/from the Router SessionRef here and workspace-model is used by no Router crate",
      },
      note: "JSON-in-string identity key column of a central-only type",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_admission.rs",
      disposition: { kind: "none" },
      note: "admit_workspace (:8) machine/workspace admission; decodes allowed_workspaces JSON array column (:34); central storage only",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_change_records.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central workspace change row, not one of the four protocols: StoredChange (workspace_change_records.rs:10) decodes SQL-built change JSON and maps change_kind strings to workspace_model ChangeEvent (36-81), decoding role names via from_value(Value::String) (53, 57)",
      },
      note: "workspace_model only",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_decode_error.rs",
      disposition: { kind: "none" },
      note: "corrupt (:2) StoreFailure helper",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_directory_rows.rs",
      disposition: { kind: "none" },
      note: "workspaces directory row decode/insert (:19, :90); relational columns only",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_drain.rs",
      disposition: { kind: "none" },
      note: "workspace drain scheduling (:14, :33, :135) over drain.sql rows; no serde shapes defined",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_item_reads.rs",
      disposition: { kind: "none" },
      note: "read_answer (:57) builds workspace_model WorkspaceAnswer from decoded rows; actor_ids (:229); central workspace model only",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_json_records.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central storage decoder, not one of the four protocols: decode (workspace_json_records.rs:6) re-parses JSON nested in string fields (posts.addressees, item_references.details, links.details, read_marks.scope via workspace_row_codec.rs:82-117) before from_value into workspace_model rows",
      },
      note: "JSON-in-string column decode of central-only rows",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_migrations.rs",
      disposition: { kind: "none" },
      note: "sqlx migrators for central/workspace corpora (:3, :6); test-support failing_upgrade (:12)",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_notice_plans.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central workspace drain planning, not one of the four protocols: DrainResult (workspace_notice_plans.rs:15) serde counter answer of drain_workspace; plan_workspace_notices (22) builds notification-window SQL from workspace_model changes",
      },
      note: "central drain result",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_read_queries.rs",
      disposition: { kind: "none" },
      note: "read_statement (:8) selects workspace_sql query and binds serde_json::to_string of addressees (:152) and of the read WorkspaceOperation (:160, consumed by json_extract in views.sql); central workspace model only",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_request_gate.rs",
      disposition: { kind: "none" },
      note: "execute (:27) runs workspace_model WorkspaceRequest; maps Caller::Session(SessionAddress) to Identity::AgentSession (:57-63), same workspace_model type, no Router SessionRef",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_request_log.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central request replay log, not one of the four protocols: retain stores serde_json::to_string(answer) in request_log.answer (workspace_request_log.rs:56) and replay decodes WorkspaceResult from it (37); digest canonicalizes the operation (8-11); workspace_model only",
      },
      note: "JSON-in-string replay column of central-only type",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_row_codec.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central workspace row decoder, not one of the four protocols: WorkspaceRows::decode (workspace_row_codec.rs:60) parses SQL-built json_object payloads per row_kind into workspace_model types (66-119)",
      },
      note: "column/string/json helpers (:6, :22, :32)",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_rule_dispatch.rs",
      disposition: { kind: "none" },
      note: "resolve_command (:13) maps workspace_model WorkspaceOperation to WorkspaceCommand, minting ids; central workspace model only",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_rule_inputs.rs",
      disposition: { kind: "none" },
      note: "run_rule (:7) feeds decoded rows into workspace_rules; no serde",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_runtime.rs",
      disposition: { kind: "none" },
      note: "WorkspaceRuntime/WorkspaceCaller/WorkspaceStoreCalls (:11, :20, :28); no serde",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_sql/backlog.sql",
      disposition: { kind: "excluded", reason: workspaceReadSqlReason },
      note: "row kinds area, backlog, highWater",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_sql/dependencies.sql",
      disposition: { kind: "excluded", reason: workspaceReadSqlReason },
      note: "row kinds project, deliverable, dependency, highWater",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_sql/drain.sql",
      disposition: { kind: "excluded", reason: workspaceReadSqlReason },
      note: "row kinds change, thread, member, post, projectRole, deliverableRole, highWater (read by workspace_drain.rs:98)",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_sql/organization.sql",
      disposition: { kind: "excluded", reason: workspaceReadSqlReason },
      note: "row kinds area, project, initiative, deliverable, highWater",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_sql/read_marks.sql",
      disposition: { kind: "excluded", reason: workspaceReadSqlReason },
      note: "row kinds mark, highWater",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_sql/relations.sql",
      disposition: { kind: "excluded", reason: workspaceReadSqlReason },
      note: "row kinds area, project, initiative, deliverable, issue, thread, post, entry, backlog, reference, link, highWater",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_sql/roles.sql",
      disposition: { kind: "excluded", reason: workspaceReadSqlReason },
      note: "row kinds project, projectRole, deliverable, deliverableRole, highWater",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_sql/statuses.sql",
      disposition: { kind: "excluded", reason: workspaceReadSqlReason },
      note: "row kinds project, initiative, deliverable, issue, highWater",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_sql/threads.sql",
      disposition: { kind: "excluded", reason: workspaceReadSqlReason },
      note: "row kinds thread, member, area, projectRole, deliverableRole, highWater",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_sql/views.sql",
      disposition: { kind: "excluded", reason: workspaceReadSqlReason },
      note: "show/list/inbox view; reads the JSON-encoded WorkspaceOperation with json_extract(?2, ...) (bound at workspace_read_queries.rs:160)",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_sql/work_log.sql",
      disposition: { kind: "excluded", reason: workspaceReadSqlReason },
      note: "row kinds project, entry, highWater",
    },
    {
      file: "crates/collaboration-central-core/src/workspace_turns.rs",
      disposition: { kind: "none" },
      note: "WorkspaceTurns per-workspace lock (:13); no serde",
    },
  ],
  sendSites: [],
};
