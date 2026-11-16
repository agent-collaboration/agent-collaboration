import type { LedgerSection } from "../schema-audit-types.ts";

// collaboration-mcp and collaboration-protocol do not depend on collaboration-central-core
// (no Cargo.toml edge), so no type defined in this crate is an MCP tool input/output by
// itself. The one crossing found here is lease_projection.rs:161, which builds the
// message_board::LeaseVersion that reaches MCP view_work.writerObservation.leaseVersion
// (collaboration-runtime central_project_reads.rs:87-88, message-board writer_observation.rs:64).
const BOARD =
  "central board/fleet protocol between Router and central, not one of the six outside edges";
const SQL =
  "generated fleet-sync-query-gen SQL binding over central-owned tables (private storage, not one of the four protocols)";

export const section: LedgerSection = {
  root: "crates/collaboration-central-core",
  files: [
    {
      file: "crates/collaboration-central-core/src/forward_delivery.rs",
      disposition: {
        kind: "excluded",
        reason: `${BOARD}: receive_forward_call decodes the DeliverForward/ForwardDeliveryRequest object-call envelope (forward_delivery.rs:66-69) and pushes a fleet_sync_protocol ServerFrame::Request carrying BoardServerRequest::ForwardedPost onto the holder's Fleet-sync connection (forward_delivery.rs:175-187)`,
      },
      note: "SessionKey (fleet_sync_session) at :165 is the Fleet-sync peer session, not a Router SessionRef; no identity conversion",
    },
    {
      file: "crates/collaboration-central-core/src/forward_observer.rs",
      disposition: {
        kind: "excluded",
        reason:
          'conformance-feature observation (lib.rs:279-280, #[cfg(feature = "conformance")]) of central\'s machine_project_requests rows, not one of the four protocols: ForwardStateRequest, ForwardStoredRecord, ForwardStateObservation (forward_observer.rs:20,27,42) at FORWARD_STATE_PATH /admin/v1/conformance/forward-state',
      },
      note: "requester_session_id (:31) is the Fleet-sync session id copied raw from generated_project_requests, not a Router identity",
    },
    {
      file: "crates/collaboration-central-core/src/forward_registry.rs",
      disposition: {
        kind: "excluded",
        reason: `${BOARD}: registry_machine_read decodes ForwardRoute/InstallationPeerRegistry/InstallationPeerDirectory object-call envelopes (forward_registry.rs:76-99) and answers ForwardRouteResult, Option<PeerId> and InstallationPeerPageRead (:116-118); installation_peer_page builds board_sync_protocol InstallationPeerPage from enrollment rows (:246)`,
      },
      note: "InstallationPeerPage is not referenced by collaboration-mcp or collaboration-protocol",
    },
    {
      file: "crates/collaboration-central-core/src/generated_admin_imports.rs",
      disposition: { kind: "none" },
      note: `${SQL}: StoredAdminImport/StoredAdminManifest rows and read/insert/update bindings (generated_admin_imports.rs:11,207); no serde`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_bootstrap.rs",
      disposition: { kind: "none" },
      note: `${SQL}: migration-failure table init/record/read/clear and StoredMigrationFailure (generated_bootstrap.rs:9-148); no serde`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_bootstrap_probes.rs",
      disposition: { kind: "none" },
      note: `${SQL}, conformance-feature only (lib.rs:328-333): bootstrap table/checksum/version probes (generated_bootstrap_probes.rs:11-303)`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_copy_summary.rs",
      disposition: { kind: "none" },
      note: `${SQL}: store/read copy summary (database_id, record) (generated_copy_summary.rs:9-60)`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_enrollment.rs",
      disposition: { kind: "none" },
      note: `${SQL}: machine_enrollments StoredEnrollment and StoredInstallationPeer rows (generated_enrollment.rs:11,229); operation columns are durable_steps journal ids, not Router operation ids`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_fleet_adrs.rs",
      disposition: { kind: "none" },
      note: `${SQL}: StoredFleetAdr/StoredAdrRepositoryFile (generated_fleet_adrs.rs:11,141); actor_service_id/endpoint_id/session_id are pass-through String columns, the SessionRef flatten/rebuild is fleet_column_codec.rs:148 (inventoried)`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_fleet_archive.rs",
      disposition: { kind: "none" },
      note: `${SQL}: StoredArchiveReport list/insert (generated_fleet_archive.rs:11-93)`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_fleet_boards.rs",
      disposition: { kind: "none" },
      note: `${SQL}: StoredFleetBoard read/list/put/batch (generated_fleet_boards.rs:11-336); actor_* session columns pass through as String, conversion lives in fleet_column_codec.rs:148`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_fleet_feed.rs",
      disposition: { kind: "none" },
      note: `${SQL}: StoredFleetFeedState/Entry/Reader and append/page/prune (generated_fleet_feed.rs:11,213,518); actor_* session columns pass through as String, conversion lives in fleet_column_codec.rs:148`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_fleet_feed_lists.rs",
      disposition: { kind: "none" },
      note: `${SQL}: StoredFeedRepositoryRef/StoredFeedAdrFile child rows (generated_fleet_feed_lists.rs:11,176)`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_fleet_lease.rs",
      disposition: { kind: "none" },
      note: `${SQL}: StoredDirectoryLease, StoredClaimReservation, StoredClaimSettlementReceipt (generated_fleet_lease.rs:11,158,232); lease_incarnation/stored_incarnation are String columns decoded elsewhere`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_fleet_reasons.rs",
      disposition: { kind: "none" },
      note: `${SQL}: StoredDecisionReason/StoredReasonEvidence (generated_fleet_reasons.rs:11,354); actor/holder/target/named_holder service/endpoint/session String columns pass through, no conversion in this file`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_fleet_record_reads.rs",
      disposition: { kind: "none" },
      note: `${SQL}: StoredFleetProject, StoredDirectorySummary, StoredFleetRecordKey, StoredFleetNameOwner reads (generated_fleet_record_reads.rs:11,117,171,521)`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_fleet_record_writes.rs",
      disposition: { kind: "none" },
      note: `${SQL}: put_project, store_directory_summary (generated_fleet_record_writes.rs:8,62); actor_* session columns are String parameters`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_fleet_repositories.rs",
      disposition: { kind: "none" },
      note: `${SQL}: StoredRepositoryAttachment read/attach/detach (generated_fleet_repositories.rs:11-160)`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_fleet_schema.rs",
      disposition: { kind: "none" },
      note: `${SQL}, conformance-feature only (lib.rs:214-219): StoredSchemaObject sqlite_schema read (generated_fleet_schema.rs:11)`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_fleet_workspaces.rs",
      disposition: { kind: "none" },
      note: `${SQL}: StoredFleetWorkspace/StoredWorkspaceRepositoryRef (generated_fleet_workspaces.rs:11,121); actor_* session columns pass through as String`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_machine_notices.rs",
      disposition: { kind: "none" },
      note: `${SQL}: machine notice state/head/floor/append/read/prune/expiry/bounds (generated_machine_notices.rs:11-335)`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_migration_records.rs",
      disposition: { kind: "none" },
      note: `${SQL}: per-corpus migration record tables (fleet_sync_*, central_*) and StoredMigrationRecord (generated_migration_records.rs:27)`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_project_requests.rs",
      disposition: { kind: "none" },
      note: `${SQL}: StoredProjectRequest read/store/delete (generated_project_requests.rs:11-153); requester_session_id is the Fleet-sync SessionKey.session_id decoded in project_request_store.rs:83, not a Router SessionRef`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_registry_lease.rs",
      disposition: { kind: "none" },
      note: `${SQL}: StoredRegistryIncarnation read/store (generated_registry_lease.rs:11-62)`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_session_conformance.rs",
      disposition: { kind: "none" },
      note: `${SQL}, conformance-feature only (lib.rs:187-192): total_changes and SQL-failure probes (generated_session_conformance.rs:11,40)`,
    },
    {
      file: "crates/collaboration-central-core/src/generated_statements.rs",
      disposition: { kind: "none" },
      note: `${SQL}: fleet decision/receipt/creation-decision ledger (generated_statements.rs:9,108,226,296); actor_* session columns pass through as String`,
    },
    {
      file: "crates/collaboration-central-core/src/guarded_statements.rs",
      disposition: { kind: "none" },
      note: "text/exact/any helpers build central_storage GuardedStatement values (guarded_statements.rs:3-17); no message shape",
    },
    {
      file: "crates/collaboration-central-core/src/holder_delivery.rs",
      disposition: {
        kind: "excluded",
        reason: `${BOARD}: on_call decodes HolderRelease/HolderWithdrawal object-call envelopes (holder_delivery.rs:218-241) and sends fleet_sync_protocol ServerCall::Release/WithdrawRelease to the holder peer via peer_request (:243-273); conformance_holder_* probes are conformance-feature only (:70-153)`,
      },
      note: "replies are fleet_sync_protocol ReleaseAnswer/WithdrawAnswer re-encoded with serde_json (:274-281)",
    },
    {
      file: "crates/collaboration-central-core/src/holder_reply_hub.rs",
      disposition: { kind: "none" },
      note: "in-memory correlation of ClientAnswer<BoardConsumerProtocol> by (SessionKey, ConnectionId, FrameId) (holder_reply_hub.rs:17-21); Fleet-sync identities only, no serde, no Router identity",
    },
    {
      file: "crates/collaboration-central-core/src/identity_contract.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central sign-in/organization identity contract, not one of the four protocols: central_identity! macro defines PersonId, ClientId, DeviceLoginRequestId, AdmissionId, CommandId as transparent RecordId newtypes with JsonSchema (identity_contract.rs:22-51)",
      },
      note: "none of these ids is a Router SessionRef/thread/input/correlation/operation id; collaboration-mcp/protocol do not depend on this crate",
    },
    {
      file: "crates/collaboration-central-core/src/identity_contract/authorization_revisions.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private sign-in-service-to-central link (AUTHORIZATION_REVISIONS_ROUTE /internal/organization-authorization-revisions, authorization_revisions.rs:8), not one of the four protocols: OrganizationFenceState, PersonFenceState, FenceRevision, PermissionPolicyDigest, AuthorizationRevision, AuthorizationRevisionAcknowledgement (:12,20,28,65,109,141)",
      },
    },
    {
      file: "crates/collaboration-central-core/src/identity_contract/device_login_values.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central device-login (OAuth issuer) contract values, not one of the four protocols: EnrollPermission machine:enroll and AllowedWorkspaces with hand JsonSchema maxItems 100 (device_login_values.rs:10,22,44-51)",
      },
    },
    {
      file: "crates/collaboration-central-core/src/identity_contract/enrollment_records.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central machine-enrollment identity contract, not one of the four protocols: AdmissionRef, EnrollmentAuthorization, MachineEnrollment, EnrollmentRevocationReason (enrollment_records.rs:13,24,37,66)",
      },
    },
    {
      file: "crates/collaboration-central-core/src/identity_contract/identity_decisions.rs",
      disposition: {
        kind: "excluded",
        reason:
          "sign-in service decision and machine-token claim contract (OAuth/JWT side), not one of the four protocols: DecideDeviceLogin, DeviceLoginDecision, DeviceLoginRefusal, DecideMachineToken, MachineTokenDecision, MachineConnectionAdmission, PersonAdmissionRequest, OrganizationStanding, MachineTokenClaims (identity_decisions.rs:11,61,78,86,104,168,193,212,258)",
      },
      note: "uuid_v7_installation (:48) re-parses RecordId into InstallationId; fleet identity, not a Router vocabulary identity",
    },
    {
      file: "crates/collaboration-central-core/src/identity_contract/identity_lookups.rs",
      disposition: {
        kind: "excluded",
        reason:
          "organization-owner presence and lookup contract on central, not one of the four protocols: ReadMachinePresence, MachinePresenceAnswer, WhoIsLoggedIn/Result, MachineRow, SessionRow, SessionStateView, IdentityLookup, IdentityLookupResult, WorkspaceLookupResult, MachineLookupResult, CommandStatusResult (identity_lookups.rs:14,24,39,49,74,122,132,161,196,224,247,269)",
      },
      note: "SessionRow.session (:123), IdentityLookup::Session (:168) and IdentityLookupResult::Session (:202) carry message_board::SessionRef by type, no flatten or rebuild; SessionStateView has no production constructor (only tests/integration/presence_lookup_contracts.rs)",
    },
    {
      file: "crates/collaboration-central-core/src/identity_contract/organization_commands.rs",
      disposition: {
        kind: "excluded",
        reason:
          "organization-owner command contract on central, not one of the four protocols: CreateOrganization, SetOrganizationOwner, CreateWorkspace, AllowSignIn, RemoveSignIn, AllowMachine, WithdrawMachineAllowance, RevokeMachineEnrollment, RemoveSession, OrganizationCommandResult, CommandRefusal, MachineAllowance, AllowanceState (organization_commands.rs:9-101)",
      },
      note: "RemoveSession.session (:61) carries message_board::SessionRef by type, no conversion",
    },
    {
      file: "crates/collaboration-central-core/src/identity_contract/organization_provisioning.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private sign-in-to-central provisioning link (PROVISION_ORGANIZATION_ROUTE, ORGANIZATION_STATEMENT_DIGEST_ROUTE, organization_provisioning.rs:8,10), compact-JWS stamp, not one of the four protocols: OrganizationStatement, ProvisionOrganization, ProvisionOrganizationResult, ReadOrganizationStatementDigest, OrganizationStatementDigestReply (:16,71,83,90,97)",
      },
    },
    {
      file: "crates/collaboration-central-core/src/installation_peer_reads.rs",
      disposition: {
        kind: "excluded",
        reason: `${BOARD}: central object-call target central.installation-peers with InstallationPeerDirectory, InstallationPeerPageRequest, InstallationPeerPageRead (installation_peer_reads.rs:14,18,29,34-38)`,
      },
      note: "InstallationPeerPage (board_sync_protocol) is not referenced by collaboration-mcp or collaboration-protocol",
    },
    {
      file: "crates/collaboration-central-core/src/issuer_key_cache.rs",
      disposition: {
        kind: "excluded",
        reason:
          "OAuth issuer JWKS decoding, not one of the four protocols: IssuerKeyId, PublishedIssuerKeys, PublishedIssuerKey (kid/kty/crv/x/alg/use) (issuer_key_cache.rs:14,63,68) decoded by IssuerKeySet::decode (:107)",
      },
    },
    {
      file: "crates/collaboration-central-core/src/issuer_key_resolution.rs",
      disposition: { kind: "none" },
      note: "IssuerKeySource port and cache-refresh policy for issuer keys (issuer_key_resolution.rs:10-61); OAuth/JWT side, no message shape",
    },
    {
      file: "crates/collaboration-central-core/src/issuer_signature_verifier.rs",
      disposition: { kind: "none" },
      note: "Ed25519 verification of signed bytes against resolved issuer keys (issuer_signature_verifier.rs:34-57); no message shape",
    },
    {
      file: "crates/collaboration-central-core/src/key_source_capabilities.rs",
      disposition: { kind: "none" },
      note: "RawJwks body/cache_control and KeyFetchFailure (key_source_capabilities.rs:2,8); no serde",
    },
    {
      file: "crates/collaboration-central-core/src/lease_projection.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "directory_metadata hand-builds message_board::LeaseVersion from LeaseProjectionVersion (lease_projection.rs:161-171); it reaches MCP view_work writerObservation.leaseVersion. LeaseProjection/ClaimGrantedUpdate/LeaseProjectionResult (:36,193,205) are central-internal serde shapes",
    },
    {
      file: "crates/collaboration-central-core/src/lease_version.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "RegistryIncarnation (lease_version.rs:7) is a second copy of message_board::RegistryIncarnation (writer_observation.rs:12) that lease_projection.rs:167 maps into the MCP-visible LeaseVersion; LeaseContentDigest (:28) is central-internal",
    },
    {
      file: "crates/collaboration-central-core/src/lib.rs",
      disposition: { kind: "none" },
      note: "module declarations and re-exports only (lib.rs:4-542); cfg gates read here: forward_observer conformance :279, machine_login_test_control test-support :58, generated_session_conformance :187, generated_fleet_schema :214, generated_bootstrap_probes :328",
    },
    {
      file: "crates/collaboration-central-core/src/machine_enrollment.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central enrollment HTTP contract used by codex-router-cli fleet enrollment (owner-to-central), not one of the four protocols: EnrollMachine, EnrollmentDecision, RevocationDecision, EnrollmentRefusal, AdmittedMachine (machine_enrollment.rs:37,54,70,77,88)",
      },
      note: "OperationId here (:14) is the durable_steps journal operation, not a Router operation id",
    },
    {
      file: "crates/collaboration-central-core/src/machine_enrollment/admission.rs",
      disposition: { kind: "none" },
      note: "read_current_enrolled_machine decodes one machine_enrollments row by PeerId (admission.rs:13-49); no message shape",
    },
    {
      file: "crates/collaboration-central-core/src/machine_enrollment/conformance_revocation.rs",
      disposition: {
        kind: "excluded",
        reason:
          'conformance-feature owner probe (declared machine_enrollment.rs:32-33 under #[cfg(feature = "conformance")]), not one of the four protocols: RevocationControl, RevocationControlInput, RevocationEnrollmentSnapshot, RevocationObservation (conformance_revocation.rs:32,45,52,62) on central.conformance-revocation (:185)',
      },
    },
    {
      file: "crates/collaboration-central-core/src/machine_enrollment/conformance_revocation/lifecycle_pause.rs",
      disposition: {
        kind: "excluded",
        reason:
          "conformance-feature module (declared conformance_revocation.rs:22 inside the conformance-gated module, machine_enrollment.rs:32): LifecyclePauseObservation {armed, reached} (lifecycle_pause.rs:9)",
      },
    },
    {
      file: "crates/collaboration-central-core/src/machine_enrollment/readmission.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central durable task input, private journal storage, not one of the four protocols: ReadmissionInput {peer} for task central.readmit-machine (readmission.rs:12,16)",
      },
    },
    {
      file: "crates/collaboration-central-core/src/machine_enrollment/revocation.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central durable task and object-call shapes, not one of the four protocols: EndPeerSessions (central.end-peer-sessions) and RevokeInput (central.revoke-machine) (revocation.rs:17,21,25,32)",
      },
    },
    {
      file: "crates/collaboration-central-core/src/machine_enrollment_record.rs",
      disposition: { kind: "none" },
      note: "decodes machine_enrollments rows into StoredMachine/EnrollmentIdentity (machine_enrollment_record.rs:18,30,131); Serialize only under the conformance feature (:8-17) for a fingerprint; operation columns are durable_steps OperationId",
    },
    {
      file: "crates/collaboration-central-core/src/machine_login_plans.rs",
      disposition: { kind: "none" },
      note: "WorkspaceRuntime decide_device_login/decide_machine_token orchestration over central storage (machine_login_plans.rs:14-132); no message shape defined or converted",
    },
    {
      file: "crates/collaboration-central-core/src/machine_login_reads.rs",
      disposition: { kind: "none" },
      note: "reads machine_approvals/machines rows into EnrollmentFacts (machine_login_reads.rs:22-142); private storage",
    },
    {
      file: "crates/collaboration-central-core/src/machine_login_row_values.rs",
      disposition: { kind: "none" },
      note: "scalar UUIDv7/timestamp checks for central rows (machine_login_row_values.rs:11-91)",
    },
    {
      file: "crates/collaboration-central-core/src/machine_login_rows.rs",
      disposition: { kind: "none" },
      note: "decodes machines/machine_approvals rows, including allowed_workspaces JSON column, into MachineRow/StoredEnrollment (machine_login_rows.rs:22,50); private storage",
    },
    {
      file: "crates/collaboration-central-core/src/machine_login_test_control.rs",
      disposition: {
        kind: "excluded",
        reason:
          'test-support-feature control (lib.rs:58-59 #[cfg(feature = "test-support")]), not one of the four protocols: MachineLoginControl::Revoke {personId, operationId, installation, enrollmentId} (machine_login_test_control.rs:17)',
      },
      note: "operation_id is AdminOperationId (person_request), not a Router operation id",
    },
    {
      file: "crates/collaboration-central-core/src/machine_login_writes.rs",
      disposition: { kind: "none" },
      note: "guarded SQL statements for machine login decisions; encode() serializes allowed_workspaces into a JSON column (machine_login_writes.rs:87-91), private storage, no protocol shape",
    },
    {
      file: "crates/collaboration-central-core/src/machine_notice_columns.rs",
      disposition: { kind: "none" },
      note: "maps board_sync_protocol BoardNotice to/from machine notice columns (machine_notice_columns.rs:15-67), declared via #[path] in machine_notice_store.rs:2; central board protocol to private storage, not one of the four protocols",
    },
    {
      file: "crates/collaboration-central-core/src/machine_notice_contract.rs",
      disposition: {
        kind: "excluded",
        reason: `${BOARD}: durable task input MachineNoticeAppend {append, hint} wrapping board_sync_protocol BoardNotice/PositionedBoardNotice (machine_notice_contract.rs:13)`,
      },
    },
    {
      file: "crates/collaboration-central-core/src/machine_notice_delivery.rs",
      disposition: {
        kind: "excluded",
        reason: `${BOARD}: DeliverMachineNotice object-call target (machine_notice_delivery.rs:26-33), decoding BoardNotice from the envelope (:148-151) and publishing fleet_sync_protocol Notice::Consumer to the peer (:93-104)`,
      },
    },
    {
      file: "crates/collaboration-central-core/src/machine_notice_outbox.rs",
      disposition: {
        kind: "excluded",
        reason: `${BOARD}: durable task input MachineNoticeInput {installation, notice: BoardNotice} for central.fleet-notice (machine_notice_outbox.rs:11,19)`,
      },
    },
    {
      file: "crates/collaboration-central-core/src/machine_notice_store.rs",
      disposition: { kind: "none" },
      note: "per-machine notice stream append/read/prune over generated_machine_notices (machine_notice_store.rs:231-348), producing board_sync_protocol PositionedBoardNotice/NoticeRead; private storage of the central board protocol",
    },
    {
      file: "crates/collaboration-central-core/src/mail_push_dispatch.rs",
      disposition: { kind: "none" },
      note: "PushDispatcher type and bounded answer over fleet_sync_delivery PushCall/ClientAnswer (mail_push_dispatch.rs:16-54); Fleet-sync delivery, no shape defined",
    },
    {
      file: "crates/collaboration-central-core/src/mail_sender_authority.rs",
      disposition: { kind: "none" },
      note: "authorized_submitter/claim_is_authorized over fleet_sync_protocol Submitter (mail_sender_authority.rs:11-73); no message shape",
    },
    {
      file: "crates/collaboration-central-core/src/mail_sender_calls.rs",
      disposition: {
        kind: "excluded",
        reason: `${BOARD}: central.sender-mail object-call target SenderFleetMail with SenderMailRequest {session: SessionKey, request: DeliveryRequest<BoardDeliveryTypes>} (mail_sender_calls.rs:22-33)`,
      },
      note: "SessionKey is the Fleet-sync peer session, not a Router SessionRef",
    },
    {
      file: "crates/collaboration-central-core/src/mail_status_dispatch.rs",
      disposition: { kind: "none" },
      note: "status publication dispatch sending fleet_sync_protocol ServerCall::PublishStatus to the machine/project holder (mail_status_dispatch.rs:113-121); central board protocol, not an outside edge; no serde shape defined",
    },
    {
      file: "crates/collaboration-central-core/src/mail_submit_admission.rs",
      disposition: { kind: "none" },
      note: "AdmitMailSubmit transaction (mail_submit_admission.rs:23-188) validating collaboration_mail_protocol MailSender and building board_sync_protocol BoardConsumerRequest::mail_submit (:105); central board mail, no shape defined; no JSON-in-string",
    },
    {
      file: "crates/collaboration-central-core/src/mail_submit_calls.rs",
      disposition: {
        kind: "excluded",
        reason: `${BOARD}: central.mail-submit object-call target FleetMailSubmit with MailSubmitRequest {session, origin, item: DeliveryEnvelope, originEpoch} and MailSubmitReply {answer, invalidShape} (mail_submit_calls.rs:11,20,34)`,
      },
    },
    {
      file: "crates/collaboration-central-core/src/migration_bookkeeping.rs",
      disposition: { kind: "none" },
      note: "failure_time validates canonical SQLite integer timestamps on StoredMigrationFailure (migration_bookkeeping.rs:6-39); no JSON, no message shape",
    },
  ],
  sendSites: [],
};
