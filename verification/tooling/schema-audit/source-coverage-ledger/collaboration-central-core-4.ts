import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/collaboration-central-core",
  files: [
    {
      file: "crates/collaboration-central-core/src/migration_failure.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central storage-migration rollback marker, not one of the four protocols: MigrationFailure (migration_failure.rs:10) is serialized into durable_steps OperationFailure::Refused.detail (40-45) and parsed back by from_rollback (60); a private durable_steps step format between central stores, never on an outside edge",
      },
      note: "JSON-in-string of a private store type (MigrationFailure in OperationFailure.detail, :40/:60); central-internal, no Router vocabulary identity",
    },
    {
      file: "crates/collaboration-central-core/src/migration_records.rs",
      disposition: { kind: "none" },
      note: "sqlx _sqlx_migrations_* bookkeeping per migration set (MigrationSet :17, AppliedMigration :84, validate :251); no serde shapes, no protocol message",
    },
    {
      file: "crates/collaboration-central-core/src/notification_windows.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central workspace notice windows, not one of the four protocols: builds workspace_model central_mail NoticeItem/MessageBody::Notice (200-212, 253-267) from StoredChange JSON assembled in SQL (165-171) and decodes NoticeCadence through serde_json::from_value of a string column (56-59); central mail is the workspace_model central design (no Router crate depends on workspace-model), not MCP/ACP/Codex",
      },
      note: "SQL-built StoredChange JSON decode (:165-171) and cadence string->serde (:56); workspace_model only, no SessionRef",
    },
    {
      file: "crates/collaboration-central-core/src/object_bootstrap.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object migration runner, not one of the four protocols: EngineSchema serde selector (object_bootstrap.rs:76) for central object bootstrap calls and the migration corpora (SESSION/REGISTRY/FLEET_MIGRATIONS 322-448); private storage schema work",
      },
      note: "EngineSchema (:76) central bootstrap selector; CopyMigrationResult built for owner admin HTTP (:154-202)",
    },
    {
      file: "crates/collaboration-central-core/src/object_bootstrap/conformance.rs",
      disposition: {
        kind: "excluded",
        reason:
          'conformance-only bootstrap control, compiled only under cfg(feature = "conformance") declared at object_bootstrap.rs:68; HistoryCorruption, BootstrapProbe, BootstrapCommand, BootstrapHistory, BootstrapProbeReply, BootstrapReply (conformance.rs:17-106) are central probe controls over fleet-sync stores, not one of the four protocols',
      },
      note: "feature conformance; fleet_sync_protocol probe replies only",
    },
    {
      file: "crates/collaboration-central-core/src/object_call_envelope.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object-to-object call envelope, not one of the four protocols: ObjectCallEnvelope (object_call_envelope.rs:12) carries target/request as serde_json::Value via to_value (47-48, 84) between central objects (Fleet/Copy/Registry); never crosses an outside edge",
      },
      note: "typed->Value round trip of central CallTarget requests (:47-48); central internal transport",
    },
    {
      file: "crates/collaboration-central-core/src/object_calls.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object call DTOs, not one of the four protocols: RegistryCall, RegistryResult, FleetCall, FleetResult (object_calls.rs:11-52) carry machine enrollment and fleet import between central edge and objects; consumed by codex-router-cli fleet enroll over the owner HTTP route, not MCP",
      },
      note: "central edge->object calls; enrollment/import payloads",
    },
    {
      file: "crates/collaboration-central-core/src/object_effects.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object dispatch DTO, not one of the four protocols: Dispatch<To> (object_effects.rs:6) binds an ObjectCallId and typed object call before central object transport",
      },
      note: "central object effect only",
    },
    {
      file: "crates/collaboration-central-core/src/object_host.rs",
      disposition: { kind: "none" },
      note: "ObjectHost trait (:5) over central_storage statements; no serde shapes",
    },
    {
      file: "crates/collaboration-central-core/src/object_runtime.rs",
      disposition: { kind: "none" },
      note: "ObjectRuntime/ObjectWork (:5, :10) runtime state holder; no serde shapes",
    },
    {
      file: "crates/collaboration-central-core/src/object_transport.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object transport, not one of the four protocols: serialize_object_call wraps To::call into ObjectCallEnvelope via to_value (object_transport.rs:49-58), ObjectCallEnvelope::decode (67) and on_registry_call decodes WireCall via from_value (192); ObjectCallId parsed into durable_steps OperationId (43-46) is a central step identity, not a Router vocabulary operation id",
      },
      note: "central object envelope encode/decode; durable_steps CallId from ObjectCallId (:42-48)",
    },
    {
      file: "crates/collaboration-central-core/src/organization_admin_contract.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central organization-admin HTTP answer, not one of the four protocols: WorkspaceView, OrganizationAdminResult, AdminRefusal (organization_admin_contract.rs:10, 49, 66) are the body of /organization-admin/v1/* routes (organization_signin_http.rs:45-57); collaboration-mcp does not depend on collaboration-central-core, so these are not MCP-reachable",
      },
      note: "embeds message_board WorkspaceId/ResourceName/WorkspaceState but is served only on central admin HTTP; AdmissionRefusal->AdminRefusal map (:80) is central-internal",
    },
    {
      file: "crates/collaboration-central-core/src/organization_admin_plans.rs",
      disposition: { kind: "none" },
      note: "admin_person_request (:11) plans verified admin operations; no serde shapes defined or converted",
    },
    {
      file: "crates/collaboration-central-core/src/organization_authority/fleet_statement.rs",
      disposition: { kind: "none" },
      note: "organization-stamp JWS verification (verify_signature_and_profile :91); OAuth/JWT-class, no serde shapes defined here",
    },
    {
      file: "crates/collaboration-central-core/src/organization_authority/fleet_statement_refusal.rs",
      disposition: { kind: "none" },
      note: "FleetOrganizationStatementRefusal error enum (:5), no serde",
    },
    {
      file: "crates/collaboration-central-core/src/organization_authority/fleet_statement_test_support.rs",
      disposition: {
        kind: "excluded",
        reason: "cfg(test) module declared at organization_authority/mod.rs:4",
      },
      note: "signed stamp fixtures with json! JWS header/claims (:62, :87); test only",
    },
    {
      file: "crates/collaboration-central-core/src/organization_authority/fleet_statement_wire.rs",
      disposition: {
        kind: "excluded",
        reason:
          "organization-stamp JWS decoding (JWT class), not one of the four protocols: OrganizationStatementDigest, StatementHeader, StatementClaims, StatementSeconds (fleet_statement_wire.rs:9, 39, 47, 68) parse the sign-in service's compact JWS (ParsedFleetStatement::parse :89)",
      },
      note: "JWS header/claims Deserialize for sign-in stamp",
    },
    {
      file: "crates/collaboration-central-core/src/organization_authority/identity.rs",
      disposition: { kind: "none" },
      note: "OrganizationIdentity provisioning decisions (:5, :26); no serde",
    },
    {
      file: "crates/collaboration-central-core/src/organization_authority/machine_login/decision_support.rs",
      disposition: { kind: "none" },
      note: "DecisionTime/DecisionIds (:14, :23) build OrganizationChangeRecord; no serde",
    },
    {
      file: "crates/collaboration-central-core/src/organization_authority/machine_login/device_login_decision.rs",
      disposition: { kind: "none" },
      note: "decide_device_login (:17) pure machine-login policy over identity_contract types; peer/installation string compare (:27) is fleet identity, not Router vocabulary",
    },
    {
      file: "crates/collaboration-central-core/src/organization_authority/machine_login/enrollment_records.rs",
      disposition: { kind: "none" },
      note: "MachineRow/StoredEnrollment/EnrollmentChange in-memory records (:45, :72, :122); no serde",
    },
    {
      file: "crates/collaboration-central-core/src/organization_authority/machine_login/machine_token_decision.rs",
      disposition: { kind: "none" },
      note: "decide_machine_token (:18) machine grant policy; no serde",
    },
    {
      file: "crates/collaboration-central-core/src/organization_authority/machine_login/mod.rs",
      disposition: { kind: "none" },
      note: "module wiring only",
    },
    {
      file: "crates/collaboration-central-core/src/organization_authority/machine_login/policy_fixture.rs",
      disposition: {
        kind: "excluded",
        reason: "cfg(test) module declared at organization_authority/machine_login/mod.rs:16",
      },
      note: "machine-login decision fixtures",
    },
    {
      file: "crates/collaboration-central-core/src/organization_authority/machine_login/revoke_enrollment.rs",
      disposition: { kind: "none" },
      note: 'compiled only under cfg(any(test, feature = "test-support")) at machine_login/mod.rs:6; revoke_machine_enrollment (:15) policy, no serde',
    },
    {
      file: "crates/collaboration-central-core/src/organization_authority/mod.rs",
      disposition: { kind: "none" },
      note: "module wiring and re-exports",
    },
    {
      file: "crates/collaboration-central-core/src/organization_authority/provisioning_failure.rs",
      disposition: { kind: "none" },
      note: "OrganizationProvisioningFailure error enum (:3), no serde",
    },
    {
      file: "crates/collaboration-central-core/src/organization_authority/sign_in_verification.rs",
      disposition: { kind: "none" },
      note: "SignInVerification trait (:16); no serde",
    },
    {
      file: "crates/collaboration-central-core/src/organization_authority/statement_replacement.rs",
      disposition: { kind: "none" },
      note: "decide_organization_statement_replacement (:48); no serde",
    },
    {
      file: "crates/collaboration-central-core/src/organization_authority/stored_stamp.rs",
      disposition: { kind: "none" },
      note: "StoredOrganizationStamp (:4) holder; no serde",
    },
    {
      file: "crates/collaboration-central-core/src/organization_authority_cache.rs",
      disposition: { kind: "none" },
      note: "OrganizationAuthority stamp health cache (:35); no serde",
    },
    {
      file: "crates/collaboration-central-core/src/organization_change_rows.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central organization change audit records, not one of the four protocols: OrganizationChangeKind, OrganizationChangeOrigin, ChangeRequestId, OrganizationChangeRecord (organization_change_rows.rs:15, 59, 72, 109) with JsonSchema are returned by the organization-admin ReadChangeRecords HTTP route and stored in organization_change_records; not MCP-reachable (collaboration-mcp does not depend on this crate)",
      },
      note: "admin HTTP + storage row codec (insert :138, decode :187)",
    },
    {
      file: "crates/collaboration-central-core/src/organization_provisioning_plans.rs",
      disposition: { kind: "none" },
      note: "provision_organization / read_statement_digest / deliver_authorization_revision (:50, :196, :224) over identity_contract; maps FleetOrganizationStatementRefusal->OrganizationProvisioningFailure (:71-80) central-internal",
    },
    {
      file: "crates/collaboration-central-core/src/organization_signin_http.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private sign-in-service-to-central HTTP routes (axum), not one of the four protocols: Addressed (organization_signin_http.rs:128) peeks organizationId from admin bodies (150) and ProvisioningRequestRefusal (200) is the wrong-operation body; routes carry identity_contract and OrganizationAdminResult only",
      },
      note: "axum Json handlers for provisioning, digest, revision, device-login, machine-token, admin forwarding",
    },
    {
      file: "crates/collaboration-central-core/src/organization_signin_port.rs",
      disposition: { kind: "none" },
      note: "OrganizationSignInPort trait + WorkspaceRuntime impl (:18, :52); no serde",
    },
    {
      file: "crates/collaboration-central-core/src/organization_stamp_rows.rs",
      disposition: { kind: "none" },
      note: "organization_identity row read/insert/replace (:7, :37, :61); relational columns, no serde",
    },
    {
      file: "crates/collaboration-central-core/src/organization_workspace_plans.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central organization-admin workspace creation, not one of the four protocols: workspace_view maps workspace_model WorkspaceState to message_board WorkspaceState (organization_workspace_plans.rs:50-53) for the admin HTTP WorkspaceView, and retained_result decodes the request_log answer string via from_value and compares Value pointer /workspace/name (61-71); AdminOperationId->RequestId/WorkspaceId (92-97, 155-160) are central admin ids, not Router vocabulary",
      },
      note: "admin HTTP retained JSON replay; no MCP or SessionRef",
    },
    {
      file: "crates/collaboration-central-core/src/owner_copy_calls.rs",
      disposition: { kind: "none" },
      note: "Copy-object owner calls decode CopyOperations/CopyOpsCall from ObjectCallEnvelope Values (:66, :74, :114-117); types defined in owner_operation_calls.rs; central object transport only",
    },
    {
      file: "crates/collaboration-central-core/src/owner_directory_page.rs",
      disposition: { kind: "none" },
      note: "owner directory page object call (:29) decodes ReadCopyDirectoryPage/CopyPageRequest from envelope; central owner admin only",
    },
    {
      file: "crates/collaboration-central-core/src/owner_http_routes.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central owner admin HTTP handlers (Cloudflare Access owner), not one of the four protocols: health/copies/repair/migrate (owner_http_routes.rs:37, 64, 103, 136) answer collaboration_central_admin_protocol CopyHealth/CopyMigrationPage/OwnerOperationFailure; failed (19) maps CopyUnavailableReason retry codes to OwnerOperationFailure",
      },
      note: "inline cfg(test) mod tests at :229",
    },
    {
      file: "crates/collaboration-central-core/src/owner_operation_calls.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central owner admin object calls, not one of the four protocols: CopyPageLimit, CopyPageRequest, ReadCopyDirectoryPage, CopyOperations, CopyOpsCall, CopyOpsAnswer, FleetProjectOperations (owner_operation_calls.rs:9-80) are durable_steps CallTargets between the owner edge and Fleet/Copy objects",
      },
      note: "central admin CallTarget DTOs",
    },
    {
      file: "crates/collaboration-central-core/src/owner_registry_calls.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central owner Registry object calls, not one of the four protocols: OwnerRegistryOperations, OwnerRegistryCall, OwnerRegistryAnswer, OwnerPreviewAnswer (owner_registry_calls.rs:13, 23, 36, 53) carry collaboration_central_admin_protocol move/takeover/retire between owner HTTP and the Fleet object",
      },
      note: "central admin CallTarget DTOs",
    },
    {
      file: "crates/collaboration-central-core/src/owner_registry_http.rs",
      disposition: { kind: "none" },
      note: "axum owner Registry handlers (preview :71, move_project :78, takeover :86, retire :94) pass through admin-protocol types; owner HTTP, not an outside edge",
    },
    {
      file: "crates/collaboration-central-core/src/owner_registry_runtime.rs",
      disposition: { kind: "none" },
      note: "owner_registry_call (:42) decodes OwnerRegistryOperations/Call from envelope Values (:51-54) and TakeoverPreview from journal result bytes (:99-100); central object/journal only",
    },
    {
      file: "crates/collaboration-central-core/src/owner_registry_steps.rs",
      disposition: { kind: "none" },
      note: "PrepareOwnerTransfer/ReadOwnerTransfer/CheckOwnerPreview steps (:25, :195, :267) map fleet HolderChangeAnswer->OwnerTransferAnswer (:224-260); fleet lease vocabulary, not the four protocols",
    },
    {
      file: "crates/collaboration-central-core/src/owner_retirement.rs",
      disposition: { kind: "none" },
      note: "owner_retire (:40) decodes RetirementAnswer from journal bytes (:96) and parses durable_steps intent.target JSON string as (String, Value) (:191-192); central journal internals, no protocol message",
    },
    {
      file: "crates/collaboration-central-core/src/pause_points.rs",
      disposition: { kind: "none" },
      note: 'compiled only under cfg(feature = "test-support") at lib.rs:13; PausePoint/PausePoints test seam (:15, :33), no serde',
    },
    {
      file: "crates/collaboration-central-core/src/peer_request.rs",
      disposition: { kind: "none" },
      note: "begin_peer_request (:22) serializes fleet_sync ServerFrame::Request (:44-47) and sends it on the central fleet-sync socket (host.send_frame :114); central<->Router fleet protocol, not one of the six outside edges, so no sendSites row",
    },
    {
      file: "crates/collaboration-central-core/src/person_admission_plans.rs",
      disposition: { kind: "none" },
      note: "prepare_person_admission (:26) writes organization_used_requests row; relational columns only",
    },
    {
      file: "crates/collaboration-central-core/src/person_request/admin_operations.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central organization-admin request bodies and signed intent, not one of the four protocols: AdminOperationId, WorkspaceName, RecordPageLimit, CreateWorkspaceRequest, WorkspaceLookupRequest, ReadChangeRecordsRequest, AdminRoute, AdminOperation, OrganizationAdminIntent(Wire) (admin_operations.rs:15-251) are parsed from /organization-admin/v1/* bodies (AdminOperation::parse :303) and canonicalized for the DPoP intent digest (canonical_json :415)",
      },
      note: "AdminOperationId is a central admin operation id, not the Router vocabulary operation id",
    },
    {
      file: "crates/collaboration-central-core/src/person_request/compact_jwt.rs",
      disposition: {
        kind: "excluded",
        reason:
          "compact JWT parsing helpers (OAuth/JWT class), not one of the four protocols: CompactJwt::parse (compact_jwt.rs:18), from_json (52), uuid_v7 (63)",
      },
      note: "JWT/base64url helpers",
    },
    {
      file: "crates/collaboration-central-core/src/person_request/floor_delivery.rs",
      disposition: { kind: "none" },
      note: "decide_floor_delivery (:39) maps identity_contract fence states to floor states (:24-37); central authorization policy, no serde",
    },
    {
      file: "crates/collaboration-central-core/src/person_request/mod.rs",
      disposition: { kind: "none" },
      note: "module wiring and re-exports",
    },
    {
      file: "crates/collaboration-central-core/src/person_request/refusal.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central person-proof refusal enum (OAuth/DPoP class), not one of the four protocols: PersonRequestRefusal (refusal.rs:6) Serialize/Deserialize for admin-route verification failures",
      },
      note: "DPoP/JWT verification outcomes",
    },
    {
      file: "crates/collaboration-central-core/src/person_request/request_admission.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central person-request admission policy, not one of the four protocols: AdmissionRefusal (request_admission.rs:81) serde enum mapped into AdminRefusal for the organization-admin HTTP answer; floors/revisions are central authorization state",
      },
      note: "decide_admission (:105) policy",
    },
    {
      file: "crates/collaboration-central-core/src/person_request/request_verifier.rs",
      disposition: {
        kind: "excluded",
        reason:
          "DPoP person-request verification (OAuth/JWT class), not one of the four protocols: ProofJwk, PersonRequestInput, VerifiedPersonRequest, TokenHeader, PersonClaims, ProofHeader, ProofClaims (request_verifier.rs:34-157) decode the sign-in access token and DPoP proof for organization-admin routes",
      },
      note: "JWT claim projections; session_id here is the sign-in session, not a Router SessionRef",
    },
    {
      file: "crates/collaboration-central-core/src/platform_capabilities.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central platform host capabilities, not one of the four protocols: UpgradeFailure (platform_capabilities.rs:26) Serialize-only socket-upgrade result; CentralPlatform/LegacyObjectHost traits (31, 61) abstract fleet-sync socket send/close (97-106)",
      },
      note: "host trait surface for Cloudflare/Tokio central",
    },
    {
      file: "crates/collaboration-central-core/src/project_request_store.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central forwarded-post request bookkeeping, not one of the four protocols: ProjectRequest, ProjectRequestPhase (project_request_store.rs:19, 37) store fleet FrameId/ConnectionId/SessionKey(peer, session_id) and message_board ProjectId in machine_project_requests columns (decode_request :43, save :149); SessionKey is the fleet-sync machine session, not the Router SessionRef",
      },
      note: "fleet SessionKey flatten/rebuild to peer/session columns (:79-86, :169-170) is fleet-sync identity, not Router vocabulary",
    },
    {
      file: "crates/collaboration-central-core/src/registration_refusal.rs",
      disposition: { kind: "converts", owners: ["mcp"] },
      note: 'render (:6) turns message_board::Refusal into fleet GateReason: code read from the serde tag via to_value(...).get("kind") (:7-11) and, for structured variants, detail = serde_json::to_string(Refusal) (:31); the Router copies GateReason.code/detail verbatim into message_board ProjectRegistrationRejection (collaboration-service/src/project_admission/project_rejection.rs:12-15), which is in MCP WriteReply output schemas (collaboration-mcp/src/mcp_server/snapshots/main_success_schemas.json) — JSON-in-string reaching MCP; inline cfg(test) tests at :40',
    },
    {
      file: "crates/collaboration-central-core/src/registry_lease_calls.rs",
      disposition: { kind: "none" },
      note: "lease_call (:49) decodes SessionLeaseRegistry/SessionLeaseRequest from envelope Values (:64-67) and LeaseAnswer from journal bytes (:127); fleet lease protocol between central objects, not the four protocols",
    },
    {
      file: "crates/collaboration-central-core/src/registry_lease_store.rs",
      disposition: { kind: "none" },
      note: "RegistryLeaseStore projection read (:19, :74); no serde",
    },
    {
      file: "crates/collaboration-central-core/src/registry_peer_lookup.rs",
      disposition: { kind: "none" },
      note: "service_for_peer (:80) fleet PeerId -> message_board ServiceId via enrollment rows; fleet identity lookup, not a SessionRef conversion",
    },
    {
      file: "crates/collaboration-central-core/src/registry_session_calls.rs",
      disposition: { kind: "none" },
      note: "session_call (:51) decodes ReportSchemaSupport/PeerId from envelope Values (:66-80) and answers fleet SchemaSupportRecorded/LeaseStatus; fleet-sync between central objects",
    },
    {
      file: "crates/collaboration-central-core/src/session_calls.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central Fleet-object call targets, not one of the four protocols: SessionLeaseRegistry, SessionLeaseRequest, WelcomeLeases (session_calls.rs:7, 10, 21) carry fleet SessionKey + LeaseRequest/PeerId between the central session object and the Fleet object",
      },
      note: "fleet-sync lease calls",
    },
    {
      file: "crates/collaboration-central-core/src/session_finalize_fault.rs",
      disposition: {
        kind: "excluded",
        reason:
          'conformance-only fault injection, compiled only under cfg(feature = "conformance") declared at lib.rs:195; SessionFinalizeFault, SessionFinalizeFailure, RollbackObservation (session_finalize_fault.rs:13, 25, 44) are JSON-in-string carried in StoreError text (store_error :74, from_store_error :55-61), central conformance only',
      },
      note: "JSON-in-string inside StoreError for conformance observation",
    },
    {
      file: "crates/collaboration-central-core/src/session_request_admission.rs",
      disposition: { kind: "none" },
      note: "refuse_register_from_another_peer (:7) fleet lease guard; inline cfg(test) tests at :21",
    },
    {
      file: "crates/collaboration-central-core/src/session_revocation.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central fleet-sync session revocation task, not one of the four protocols: EndSessionsInput (session_revocation.rs:26) durable_steps Task input for central.end-sessions (46); end_peer_sessions (104) decodes EndPeerSessions from envelope Values",
      },
      note: "fleet session lifecycle",
    },
    {
      file: "crates/collaboration-central-core/src/session_runtime.rs",
      disposition: { kind: "none" },
      note: "on_socket_opened/on_frame/on_socket_closed (:88, :109, :178) drive fleet_sync_session over central sockets; no serde shapes defined",
    },
    {
      file: "crates/collaboration-central-core/src/session_scope.rs",
      disposition: { kind: "none" },
      note: "CoreSessionScope (:29) stages fleet ServerFrame answers (serde_json::to_vec :198, :239, :390) onto central fleet-sync sockets; central<->Router fleet protocol, not one of the six outside edges",
    },
    {
      file: "crates/collaboration-central-core/src/session_scope/enrollment_gate.rs",
      disposition: { kind: "none" },
      note: "refuse_unenrolled_mutation (:28) builds fleet refusals with GateReason/HookRefusal code callerNotEnrolled (:35-37, :71-74) and BoardPayload::encode of fleet_admission results (:79, :87); fleet-sync answers (GateReason reaches the Router as RegistrationRefusal::DatabaseRejected, plain text detail, see registration_refusal.rs note)",
    },
  ],
  sendSites: [],
};
