import type { LedgerSection } from "../schema-audit-types.ts";

/*
 * Shared ground for every reason below (checked at afc76c82c): collaboration-mcp and
 * collaboration-protocol do not depend on collaboration-central-core (their Cargo.toml has it in
 * neither [dependencies] nor [dev-dependencies]; collaboration-mcp only dev-depends on
 * board-central-test-support), so no type in these files is reachable from an MCP tool's input or
 * output. workspace_model::SessionAddress (workspace-model/src/entity_records.rs:81) is the central
 * workspace's session address, used only by central crates and central-link-*, not
 * message_board::SessionRef; none of these files names SessionRef, a Codex thread id, an input id
 * or a correlation id.
 */
export const section: LedgerSection = {
  root: "crates/collaboration-central-core",
  files: [
    {
      file: "crates/collaboration-central-core/src/access_contract.rs",
      disposition: {
        kind: "excluded",
        reason:
          "Cloudflare Access JWT/JWKS policy for central HTTP admission (OAuth/JWT, not one of the four protocols): AccessClientId (access_contract.rs:7), AccessCaller (:34), AccessRefusal (:40), AccessPolicy (:74), AccessJwk (:143), AccessJwks (:153); also used by codex-router-cli central/fleet enrollment commands, not a Codex, ACP or MCP edge",
      },
      note: "serde newtype/enums/structs of the Access verifier; no Router identity conversion",
    },
    {
      file: "crates/collaboration-central-core/src/access_key_cache.rs",
      disposition: {
        kind: "excluded",
        reason:
          "decodes the Cloudflare Access JWKS HTTP response body into AccessJwks (access_key_cache.rs:39) and parses its Cache-Control max-age (:41-50); Access key fetch, not one of the four protocols",
      },
      note: "AccessKeyCache::remember (:33) is the only decode",
    },
    {
      file: "crates/collaboration-central-core/src/access_key_profile.rs",
      disposition: { kind: "none" },
      note: "validate_access_key_profile (:6) checks RSA modulus/exponent of an AccessJwk; no message shape",
    },
    {
      file: "crates/collaboration-central-core/src/access_observation.rs",
      disposition: { kind: "none" },
      note: "in-process observation enums without serde: AccessRouteClass (:6), AccessKeySource (:25), AccessCheckOutcome (:32), CentralObservation (:39)",
    },
    {
      file: "crates/collaboration-central-core/src/access_rules.rs",
      disposition: {
        kind: "excluded",
        reason:
          "JWT assertion parsing for Cloudflare Access (OAuth/JWT, not one of the four protocols): AssertionHeader (access_rules.rs:8), Audience (:17), AssertionClaims (:30) decoded at :72 and :80; authorize_access_route (:143) gates /v1/session and /admin/v1/ by caller class",
      },
      note: "ParsedAccessAssertion::parse (:52) builds AccessCaller from claims",
    },
    {
      file: "crates/collaboration-central-core/src/admin_import_columns.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private central storage: decodes admin_imports / admin manifest SQL rows into AdminImportRecord with fleet_sync_protocol SnapshotManifest and board_sync_protocol BoardAnnotation (ImportRoot::decode :67, finish :254, decode_time :324); not one of the four protocols",
      },
      note: "read (:20) is the only entry; phase kinds reserved/published/complete decoded at :172-201",
    },
    {
      file: "crates/collaboration-central-core/src/admin_import_contract.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central owner admin HTTP import protocol (POST /admin/v1/import/projects/{id}, central_router.rs:57), not one of the six outside edges: AdminImportHeader (admin_import_contract.rs:13), AdminImportPhase (:30), AdminImportRecord (:38), AdminProjectImportRequest (:52), AdminProjectImportAnswer (:80), AdminImportRefusal (:143)",
      },
      note: "payloads are fleet_sync_protocol / board_sync_protocol / fleet_admission types",
    },
    {
      file: "crates/collaboration-central-core/src/admin_import_copy.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object-call targets for admin seeding, not one of the four protocols: AdminCopySeed (admin_import_copy.rs:25, KIND central.admin-copy-seed), AdminSeedRequest (:40), AdminSeedAnswer (:60), FleetAdminCopySeed (:280); decodes ObjectCallEnvelope target/request (:95-98) and re-encodes a CopyAdminSeed call (:258-263)",
      },
      note: "to_value/from_value here is encode/decode of the same central call type into the envelope, not an identity conversion",
    },
    {
      file: "crates/collaboration-central-core/src/admin_import_fleet.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object-call target AdminImportWorkflow (admin_import_fleet.rs:22, KIND central.admin-import-workflow) decoded with its AdminImportCommand request at :47-49; central admin import workflow, not one of the four protocols",
      },
      note: "answer serialized with serde_json::to_vec at :72",
    },
    {
      file: "crates/collaboration-central-core/src/admin_import_registry.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object-call target AdminImportReservation (admin_import_registry.rs:26) and ReservationAnswer (:39) wrapping fleet_sync_protocol Import/ImportAnswer (decoded :70, :98); fleet library reservation, not one of the four protocols",
      },
      note: "AdmitImport step (:107) admits fleet_sync_lease ImportTask",
    },
    {
      file: "crates/collaboration-central-core/src/admin_import_routes.rs",
      disposition: {
        kind: "excluded",
        reason:
          "axum owner admin HTTP handlers: fleet (admin_import_routes.rs:28) decodes fleet_admission::FleetImportBundle, project (:48) decodes AdminProjectImportRequest; copy_failure (:338) hand-maps fleet_sync_protocol ReplicationAnswer to AdminProjectImportAnswer; central admin protocol, not one of the four protocols",
      },
      note: "invoke (:361) encodes central object calls via encode_shared_call",
    },
    {
      file: "crates/collaboration-central-core/src/admin_import_store.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object-call request AdminImportCommand (admin_import_store.rs:25) of the admin import workflow; central admin protocol, not one of the four protocols",
      },
      note: "AdminImportStep (:66) applies Reserve/Read/Published/Complete to stored records",
    },
    {
      file: "crates/collaboration-central-core/src/admin_import_writes.rs",
      disposition: { kind: "none" },
      note: "SQL inserts/updates of admin_imports rows (insert :10, update_phase :83); private storage, no serde shape",
    },
    {
      file: "crates/collaboration-central-core/src/authorization_floor_rows.rs",
      disposition: { kind: "none" },
      note: "SQL row decode and guard statements for authorization floors (decode_organization_floor :35, decode_person_floor :68); no serde shape",
    },
    {
      file: "crates/collaboration-central-core/src/authorization_revision_plans.rs",
      disposition: { kind: "none" },
      note: "prepare_authorization_revision (:22) builds guarded SQL from AuthorizationRevision; no serde shape",
    },
    {
      file: "crates/collaboration-central-core/src/call_edge.rs",
      disposition: {
        kind: "excluded",
        reason:
          "CallEdgeRefused (call_edge.rs:6), the serde error of the central object-call DAG; not one of the four protocols",
      },
      note: "validate_call_edge (:10)",
    },
    {
      file: "crates/collaboration-central-core/src/call_graph.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object-call graph, not one of the four protocols: CallerKind (call_graph.rs:9), object_kind! marker structs (:68), EdgeCall/EdgeResult/SharedCallOnly/SharedAnswerOnly (:84-90)",
      },
      note: "allowed_edges! (:134) defines CallerKind::may_call",
    },
    {
      file: "crates/collaboration-central-core/src/caller_context.rs",
      disposition: {
        kind: "excluded",
        reason:
          "CallAuthority (caller_context.rs:33), the serde authority carried inside central ObjectCallEnvelope; not one of the four protocols",
      },
      note: "CallerContext::for_verified_edge (:50) maps AccessCaller to CallAuthority",
    },
    {
      file: "crates/collaboration-central-core/src/central_application.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central Access verification and object-call transport: prepare_call (central_application.rs:59) and call_from_edge (:74) serialize central object calls, call_from_owner (:105) decodes the reply at :139; not one of the four protocols",
      },
      note: "verify_assertion (:150) fetches JWKS and verifies RS256",
    },
    {
      file: "crates/collaboration-central-core/src/central_mail_records.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private central storage: decode_messages (central_mail_records.rs:29) decodes JSON TEXT columns of the messages table into workspace_model central_mail MessageSender (:36), SessionAddress (:37) and MessageBody (:48); SessionAddress is the central workspace address, not message_board::SessionRef, so no Router identity conversion",
      },
      note: "queued_for_session (:119) reads queued messages for one SessionAddress",
    },
    {
      file: "crates/collaboration-central-core/src/central_mail_requests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central mail protocol handler: request_central (central_mail_requests.rs:12) takes workspace_model::central_mail::CentralMailRequest/Result (Router central link to central, not one of the six outside edges); reads cached answer JSON from request_log (:61) and stores the notice cadence via serde_json::to_value(cadence).as_str (:100-102)",
      },
      note: "SendMessage queues a Direct message (:134-151); timers go to plan_timer_request",
    },
    {
      file: "crates/collaboration-central-core/src/central_mail_store.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private central storage: encode (central_mail_store.rs:10) serializes MessageSender, SessionAddress and MessageBody into messages TEXT columns (:51-55); known_session (:89) matches canonical JSON of workspace_model Identity::AgentSession (:94); not one of the four protocols",
      },
      note: "EARLIEST_DUE alarm query (:18)",
    },
    {
      file: "crates/collaboration-central-core/src/central_mail_time.rs",
      disposition: { kind: "none" },
      note: "five-minute grid and seven-day lifetime checks for central wakes/automations (refusal :147); builds CentralMailRefusal values, no serde shape defined or decoded",
    },
    {
      file: "crates/collaboration-central-core/src/central_organization_identity.rs",
      disposition: { kind: "none" },
      note: "SQL read of organization_identity into CentralOrganizationIdentity (:6, no serde)",
    },
    {
      file: "crates/collaboration-central-core/src/central_router.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central HTTP route table and fleet-sync session upgrade, not one of the six outside edges: EnrollmentBody (central_router.rs:158), RevocationBody (:169), EdgeFailure (:202); open_session (:240) upgrades /v1/session to the fleet-sync session; credential_refusal (:303) writes AuthRefusal JSON into the fleet-sync-auth-refusal header",
      },
      note: "routes :31-128; conformance routes only under feature conformance (:79)",
    },
    {
      file: "crates/collaboration-central-core/src/central_timer_plans.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private central storage: decode_wakes (central_timer_plans.rs:42) and decode_automations (:84) decode SessionAddress target JSON (:48, :163), state via serde_json::from_value(Value::String) (:65, :88) and TimingRule JSON (:106); plan_timer_request (:179) encodes target/timing (:208, :305-306); not one of the four protocols",
      },
      note: "SessionAddress is the central workspace address, not SessionRef",
    },
    {
      file: "crates/collaboration-central-core/src/central_timers.rs",
      disposition: {
        kind: "excluded",
        reason:
          "AlarmResult (central_timers.rs:14), the serde summary of one central alarm; on_alarm (:71) fires wakes/automations as central mail rows; not one of the four protocols",
      },
      note: "validated_timers (:35)",
    },
    {
      file: "crates/collaboration-central-core/src/claim_reservation_contract.rs",
      disposition: {
        kind: "excluded",
        reason:
          "fleet lease settlement state ClaimReservationState (claim_reservation_contract.rs:10) and ClaimReservationOutcome (:17); central fleet protocol, not one of the four protocols",
      },
    },
    {
      file: "crates/collaboration-central-core/src/clock_ids.rs",
      disposition: { kind: "none" },
      note: "ClockIds (:11) mints UUIDv7 from the injected clock; no message shape",
    },
    {
      file: "crates/collaboration-central-core/src/conformance_access.rs",
      disposition: {
        kind: "excluded",
        reason:
          "owner-gated central conformance probe, compiled only with feature conformance (lib.rs:41-42), POST /admin/v1/conformance/access: AccessVerificationOutcome (conformance_access.rs:20), AccessCallerClass (:27), VerifyAccessInput (:45); not one of the four protocols",
      },
    },
    {
      file: "crates/collaboration-central-core/src/conformance_copy.rs",
      disposition: {
        kind: "excluded",
        reason:
          "owner-gated central conformance probe, feature conformance (lib.rs:452-453), /admin/v1/conformance/copy-*: CopyProbeInput (conformance_copy.rs:38), CopyProbeRequest (:50), CopyProbeReply (:71), CopyProbe (:80), CopyProbeStatus (:87), ObserveDriveTokens (:95); not one of the four protocols",
      },
      note: "copy_probe (:245) decodes the object-call envelope",
    },
    {
      file: "crates/collaboration-central-core/src/conformance_copy_wait.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central conformance wait probe, feature conformance (lib.rs:454-455): CopyWaitTarget (conformance_copy_wait.rs:10), CopyWaitInput (:22), ControlCopyWait (:28), CopyWaitControl (:37), CopyWaitObservation (:45); not one of the four protocols",
      },
    },
    {
      file: "crates/collaboration-central-core/src/conformance_drive_tokens.rs",
      disposition: { kind: "none" },
      note: "feature conformance (lib.rs:456-457); DriveTokenObserver (:6) wraps AtomicStore; no serde shape",
    },
    {
      file: "crates/collaboration-central-core/src/conformance_fleet.rs",
      disposition: {
        kind: "excluded",
        reason:
          "owner-gated central conformance fixture, feature conformance (lib.rs:43-44): FleetReceiptProbe (conformance_fleet.rs:26) with StepControl request decoded at :97; not one of the four protocols",
      },
    },
    {
      file: "crates/collaboration-central-core/src/conformance_fleet_entity.rs",
      disposition: { kind: "none" },
      note: "feature conformance (lib.rs:45-46); WriteFleetEntityFixture (:15) writes a message_board Record through fleet_record_store; no serde shape defined or decoded",
    },
    {
      file: "crates/collaboration-central-core/src/conformance_holder.rs",
      disposition: {
        kind: "excluded",
        reason:
          "owner-gated central conformance probe, feature conformance (lib.rs:465-466): HolderProbeInput (conformance_holder.rs:37), HolderProbeRequest (:49), HolderProbeReply (:60), ProbeAnswer (:71), ObserveRegistryDispatches (:81), HolderProbe (:88), HolderProbeStatus (:95); not one of the four protocols",
      },
    },
    {
      file: "crates/collaboration-central-core/src/connection_admission.rs",
      disposition: { kind: "none" },
      note: "re-exports message_board::ActorOrigin as AdmissionOrigin (:3); defines nothing",
    },
    {
      file: "crates/collaboration-central-core/src/copy_call_failure.rs",
      disposition: { kind: "none" },
      note: "maps durable_steps CallFailure to fleet_sync_protocol ReplicationAnswer (answer :15); central fleet-sync failure kinds, no serde shape",
    },
    {
      file: "crates/collaboration-central-core/src/copy_calls.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central Copy object call dispatch: on_call (copy_calls.rs:43) decodes ObjectCallEnvelope and fleet_sync_lease targets by KIND; owner_project_call (:204) re-targets the envelope with serde_json::to_value (:229-275) and to_vec (:287); not one of the four protocols",
      },
    },
    {
      file: "crates/collaboration-central-core/src/copy_change_notifier.rs",
      disposition: { kind: "none" },
      note: "in-process commit notifier (CopyChangeNotifier :17); no message shape",
    },
    {
      file: "crates/collaboration-central-core/src/copy_directory_summary.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object-call target DirectorySummaryUpdate (copy_directory_summary.rs:24) with CopyDirectorySummary request decoded at :49; stores board_sync_protocol BoardAnnotation summary columns (:114-136) and decodes stored summary JSON (:193); not one of the four protocols",
      },
    },
    {
      file: "crates/collaboration-central-core/src/copy_health.rs",
      disposition: {
        kind: "excluded",
        reason:
          "builds the central admin protocol CopyHealth / LiveCopyHealth (collaboration_central_admin_protocol, copy_health.rs:10-12) for /admin/v1/copies/{id}/health, hand-mapping fleet_sync_receiver CopyStage (:93-97); not one of the four protocols",
      },
    },
    {
      file: "crates/collaboration-central-core/src/copy_mutations.rs",
      disposition: { kind: "none" },
      note: "in-process CopyMutation (:18) / CopyMutationAnswer (:24) without serde",
    },
    {
      file: "crates/collaboration-central-core/src/copy_read_calls.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central Session-to-Copy object call: CopySessionRead (copy_read_calls.rs:22) and CopyReadRequest (:35) answered with fleet_sync_protocol ReplicationAnswer; not one of the four protocols",
      },
    },
    {
      file: "crates/collaboration-central-core/src/copy_receiver_rules.rs",
      disposition: {
        kind: "excluded",
        reason:
          "CopyDirectorySummary (copy_receiver_rules.rs:14) wrapping board_sync_protocol BoardAnnotation, serialized into the copy summary column (:67); central copy storage, not one of the four protocols",
      },
    },
    {
      file: "crates/collaboration-central-core/src/copy_runtime.rs",
      disposition: { kind: "none" },
      note: "copy status read and acked wait (await_copy_acked :76); no serde shape",
    },
    {
      file: "crates/collaboration-central-core/src/copy_staged_parts.rs",
      disposition: { kind: "none" },
      note: "one-day staged-part retention (ExpireStagedParts :56); no message shape",
    },
    {
      file: "crates/collaboration-central-core/src/copy_write_calls.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object-call targets for copy writes, not one of the four protocols: CopySessionWrite (copy_write_calls.rs:32), WriterCopyRequest (:43), CopyAdminSeed (:50), AdminCopySeedRequest (:67), FleetProjectSeed (:463); CopyWrite::writer (:107) hand-maps fleet_sync_protocol ReplicationRequest to CopyWrite",
      },
    },
    {
      file: "crates/collaboration-central-core/src/core_contract.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object-call addressing, not one of the four protocols: ObjectCallId (core_contract.rs:8), RegistryTime (:32), ObjectAddress (:36)",
      },
    },
    {
      file: "crates/collaboration-central-core/src/core_host.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central shared-task Host: classify (core_host.rs:276) routes central object calls; dispatch (:465) re-decodes generic call types with serde_json::to_value/from_value for fleet_sync_delivery PushCall/PublishCall<BoardDeliveryTypes> (:475-478, :509-512), RegistrationAdmission (:481-486), ClaimSettlement (:489-497) and FollowUpCall (:501-503); central fleet-sync calls, no Router identity, not one of the four protocols",
      },
      note: "gather_welcome (:192) builds fleet_sync_protocol DatabaseStatus",
    },
    {
      file: "crates/collaboration-central-core/src/core_host/copy_followup.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central copy follow-up: decodes FollowUpCall<CopyDirectorySummary> and its item (copy_followup.rs:44-45), returns FollowUpAnswer via to_value (:54); not one of the four protocols",
      },
    },
    {
      file: "crates/collaboration-central-core/src/core_host/imported_board_snapshot.rs",
      disposition: {
        kind: "excluded",
        reason:
          'conformance-only fixture, module declared under #[cfg(feature = "conformance")] at core_host.rs:550-551; conformance_import_board_snapshot (imported_board_snapshot.rs:34) decodes fleet_sync_protocol ImportAnswer (:67) and drives admin seed calls; not one of the four protocols',
      },
    },
    {
      file: "crates/collaboration-central-core/src/core_host/imported_unheld_fixture.rs",
      disposition: {
        kind: "excluded",
        reason:
          'conformance-only fixture, module declared under #[cfg(feature = "conformance")] at core_host.rs:545-546: ConformanceBoardState (imported_unheld_fixture.rs:24); fixture_fleet (:231) builds a fleet_admission FleetImportBundle; not one of the four protocols',
      },
    },
    {
      file: "crates/collaboration-central-core/src/core_host/lease_settlement.rs",
      disposition: {
        kind: "excluded",
        reason:
          "fleet lease settlement: settle_granted_claim (lease_settlement.rs:23) answers fleet_sync_protocol SettlementRecorded via to_value (:66); not one of the four protocols",
      },
    },
    {
      file: "crates/collaboration-central-core/src/core_host/registration_gate.rs",
      disposition: {
        kind: "excluded",
        reason:
          "fleet registration gate: admit_registration (registration_gate.rs:9) converts fleet_sync_protocol DatabaseId to message_board::ProjectId (:17) and answers GateAnswer; board project identity inside central, not a Router session identity, not one of the four protocols",
      },
    },
    {
      file: "crates/collaboration-central-core/src/core_host/session_routes.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object-call routing for fleet-sync session callers: classify_session (session_routes.rs:8) decodes serde_json::Value target/request into SenderFleetMail, DeliverForward, ForwardRoute, InstallationPeerRegistry, FleetRead, FleetMailSubmit, WelcomeLeases, SessionLeaseRegistry, CopyRead, CopySessionWrite, CopySessionRead and re-encodes with encode_shared_call; no Codex, ACP or MCP frame",
      },
    },
    {
      file: "crates/collaboration-central-core/src/delivery_config.rs",
      disposition: {
        kind: "excluded",
        reason:
          "DeliveryConfig (delivery_config.rs:8), the relay PeerId returned to Routers at central enrollment; central fleet configuration, not one of the four protocols",
      },
    },
    {
      file: "crates/collaboration-central-core/src/edge_contract.rs",
      disposition: {
        kind: "excluded",
        reason:
          "OwnerIdentity (edge_contract.rs:3), serde newtype of the Access-verified owner email; Access/JWT, not one of the four protocols",
      },
    },
    {
      file: "crates/collaboration-central-core/src/effect_driver.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central fleet-sync session effects: serializes fleet_sync_protocol ServerFrame<BoardConsumerProtocol> Answer (effect_driver.rs:304-308) and Welcome (:346) and writes them with host.send_frame (:151, :312, :324, :366) on the central /v1/session websocket to a Router's central link; central board/fleet-sync protocol, not one of the six outside edges",
      },
    },
    {
      file: "crates/collaboration-central-core/src/effect_driver/archive_reports.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central fleet-sync session answer: BoardPayload::encode of the archive outcome (archive_reports.rs:29), ServerFrame<BoardConsumerProtocol> Answer ArchiveSeen (:40-44), send_frame (:53); central board protocol, not one of the six outside edges",
      },
    },
    {
      file: "crates/collaboration-central-core/src/effect_driver/copy_reads.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central fleet-sync session answer: ServerFrame<BoardConsumerProtocol> Answer Replication (copy_reads.rs:44-48), send_frame (:52); central fleet-sync protocol, not one of the six outside edges",
      },
    },
    {
      file: "crates/collaboration-central-core/src/effect_driver/copy_writes.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central fleet-sync session answer: ServerFrame<BoardConsumerProtocol> Answer Replication (copy_writes.rs:42-46), send_frame (:50); central fleet-sync protocol, not one of the six outside edges",
      },
    },
    {
      file: "crates/collaboration-central-core/src/effect_driver/fleet_reads.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central fleet-sync session answer: forwards message_board RequestEffectsRequest to FleetRequestEffects (fleet_reads.rs:44-56) and sends ServerFrame<BoardConsumerProtocol> Answer Read (:74-78, send_frame :82); the request type is defined in message-board, not here; central board protocol, not one of the six outside edges",
      },
    },
    {
      file: "crates/collaboration-central-core/src/effect_driver/fleet_requests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central fleet-sync session answer: BoardPayload::encode of the fleet decision (fleet_requests.rs:43), BoardConsumerAnswer::Fleet (:45) or InvalidShape (:55), ServerFrame to_vec (:62), send_frame (:70); central board protocol, not one of the six outside edges",
      },
    },
    {
      file: "crates/collaboration-central-core/src/effect_driver/forwards.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central forward delivery: dispatches ForwardRoute (forwards.rs:46) and DeliverForward (:99) object calls and hand-maps ForwardDeliveryResult to board_sync_protocol ForwardOutcome (:104-110); central board protocol, not one of the four protocols",
      },
    },
    {
      file: "crates/collaboration-central-core/src/effect_driver/installation_reads.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central fleet-sync session answer: builds board_sync_protocol InstallationPeerLookup / BoardReadPage (installation_reads.rs:40-56) and sends ServerFrame<BoardConsumerProtocol> Answer (:76, send_frame :80); central board protocol, not one of the six outside edges",
      },
    },
    {
      file: "crates/collaboration-central-core/src/effect_driver/mail_submit.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central fleet-sync session answer: FleetMailSubmit of a DeliveryEnvelope<BoardDeliveryTypes> (mail_submit.rs:38-50) and ServerFrame<BoardConsumerProtocol> Answer MailSubmit (:64-67, send_frame :72); central board mail, not one of the six outside edges",
      },
    },
    {
      file: "crates/collaboration-central-core/src/effect_driver/sender_mail.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central fleet-sync session answer: SenderFleetMail of a DeliveryRequest<BoardDeliveryTypes> (sender_mail.rs:28-35) and ServerFrame<BoardConsumerProtocol> Answer Delivery (:43-46, send_frame :51); central board mail, not one of the six outside edges",
      },
    },
    {
      file: "crates/collaboration-central-core/src/effect_execution.rs",
      disposition: { kind: "none" },
      note: "WireKind (:3) maps RegistryKind/FleetKind calls into WireCall; no serde shape defined",
    },
  ],
  sendSites: [],
};
