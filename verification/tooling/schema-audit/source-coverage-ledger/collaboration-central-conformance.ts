import type { LedgerSection } from "../schema-audit-types.ts";

const R = "crates/collaboration-central-conformance/src/";

/** The central fleet-session wire (Hello/Welcome, request/answer frames) is the central board/fleet protocol, not one of the six outside edges. */
const FLEET_FRAMES = "central fleet-session frames, not one of the six outside edges";

export const section: LedgerSection = {
  root: "crates/collaboration-central-conformance",
  files: [
    {
      file: `${R}access_expiry_scenario.rs`,
      disposition: { kind: "none" },
      note: "assertion_expiry (8) reads only the exp claim of a Cloudflare Access JWT; run_staging_expiry_row (25) checks AccessVerificationOutcome before and after expiry; JWT only",
    },
    {
      file: `${R}access_fixture.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "Cloudflare Access JWT fixtures (OAuth/JWT), not one of the four protocols: WeakKeyFixture (52-55, Deserialize) reads an AccessJwk; fixture_claims (62) and sign_assertion (83) hand-build Access JWT claims/header with json!",
      },
      note: "JwksFixtureServer (131) serves the fixture AccessJwks over loopback",
    },
    {
      file: `${R}access_scenarios.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "Cloudflare Access verifier scenarios (OAuth/JWT), not one of the four protocols: WeakAssertionFixture (65-68, Deserialize) and set_claim (256) edits of fixture JWT claims",
      },
      note: "refused_assertions (102) builds expired/wrong-audience/wrong-issuer assertions and their AccessRefusal",
    },
    {
      file: `${R}admin_import_scenario.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "central owner admin import, not one of the six outside edges: prepare (22) decodes a hand json! FleetImportBundle graph (24-31) and posts AdminProjectImportRequest to /admin/v1/import/projects (233)",
      },
      note: "fixture (135) builds fleet_sync_protocol SnapshotPage/SnapshotManifest and AdminImportHeader with a board_sync_protocol ProjectSummary annotation",
    },
    {
      file: `${R}adr_runtime.rs`,
      disposition: { kind: "none" },
      note: "run_adr_round_trip_conformance (54) writes typed fleet_admission FleetRequest AdrCreate/AdrTransition through NativeBoardSession and compares message_board Adr records; central board protocol, typed only",
    },
    {
      file: `${R}archive_runtime.rs`,
      disposition: { kind: "none" },
      note: "NativeBoardSession::report (64) sends typed board_sync_protocol ArchiveSeen; finalized_records (14) edits message_board Record envelopes for the expected snapshot; central board protocol, typed only",
    },
    {
      file: `${R}composed_client.rs`,
      disposition: {
        kind: "excluded",
        reason: `${FLEET_FRAMES}: send/receive (40, 53) write ClientFrame<BoardConsumerProtocol> and decode ServerFrame; copy_probe (73) posts hand json! {attempt, copy, request} to /admin/v1/conformance/copy-call (95-98)`,
      },
      note: "control (14) posts RuntimeControlRequest to /admin/v1/conformance/runtime; copy_probe reads the pending operation id with from_value (111)",
    },
    {
      file: `${R}composed_copy_fixture.rs`,
      disposition: {
        kind: "excluded",
        reason: `${FLEET_FRAMES}: CopyFixture (17) sends ReplicationRequest pieces over admitted sessions and hand json! copy-probe bodies {kind:create|fence,value} (31, 141)`,
      },
      note: "piece/parts/row (159-235) build fleet_sync_protocol Piece and RowChange fixtures",
    },
    {
      file: `${R}composed_copy_suite.rs`,
      disposition: { kind: "none" },
      note: "run_composed_copy_suite (15) drives typed fleet_sync_protocol ReplicationRequest/FenceRequest scenarios and RuntimeControl via the fixture; typed central fleet protocol only",
    },
    {
      file: `${R}composed_fleet_conflict.rs`,
      disposition: { kind: "none" },
      note: "write/records (48, 68) send typed BoardConsumerRequest Fleet/Read and decode FleetRequestResult and Record payloads; central board protocol, typed only",
    },
    {
      file: `${R}composed_numeric.rs`,
      disposition: {
        kind: "excluded",
        reason: `${FLEET_FRAMES}: prepare (26) posts a hand json! copy-call create body (35-40) and request (213) sends ClientFrame Replication pieces`,
      },
      note: "snapshot (108) pages ReplicationAnswer::SnapshotPage and compares SqlValue rows",
    },
    {
      file: `${R}conformance_client.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "central conformance HTTP client, not one of the six outside edges: AdmittedConnectionObservation (17-22, Deserialize) reads {connection, peer, registration} from /__conformance/admission (55)",
      },
      note: "session_connections (48) posts hand json! {kind:sessionConnections,value:{service}} (57); verify_access_assertion (209) posts {assertion, path} (219); Access header selection in as_owner/with_machine_access (140, 156)",
    },
    {
      file: `${R}enrollment_scenarios.rs`,
      disposition: {
        kind: "excluded",
        reason: `central machine enrollment admin and ${FLEET_FRAMES}: hand json! enrollMachine requests (36, 180), Hello frame (206) and Welcome comparison (223); response_peer (125) reads machineEnrolled fields by name`,
      },
      note: "binding_refusals (61) edits enrollment JSON by pointer_mut",
    },
    {
      file: `${R}feed_history_scenario.rs`,
      disposition: { kind: "none" },
      note: "feed (45) pages typed BoardConsumerRequest Read FleetRecords and decodes message_board Record; central board protocol, typed only",
    },
    {
      file: `${R}fleet_mail_observer_scenario.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "envelope (97) hand-builds the board mail recipient {peer, address:{endpoint:{serviceId,endpointId:codex-local},sessionId}} with json! (102-103) and decodes it by from_value (101) into BoardDeliveryTypes::Address = message_board::SessionRef; inventory row. Also hand json! MailSender (100), MessageBody (107-108) and mailSubmit request frame (119-120): central board mail, not one of the six outside edges",
    },
    {
      file: `${R}holder_reply_scenarios.rs`,
      disposition: {
        kind: "excluded",
        reason: `${FLEET_FRAMES}: enroll_open_peer_with_label (62) hand-builds enrollMachine (71-74), Hello/Welcome (92-94); run_holder_reply_conformance (127) builds holder-call input (163-164) and answer frame (207) with json! and reads frames by pointer (196-202)`,
      },
      note: "AdmittedPeerSocket (12) holds the socket plus its Hello and expected Welcome",
    },
    {
      file: `${R}lease_session_scenarios.rs`,
      disposition: {
        kind: "excluded",
        reason: `${FLEET_FRAMES}: hand json! enrollMachine (15-21), Hello (37), Welcome (39), lease renew request (49-51) and its NotRegistered answer (54)`,
      },
      note: "receive (75) decodes a text or binary frame as serde_json Value",
    },
    {
      file: `${R}ledger_fixture.rs`,
      disposition: { kind: "none" },
      note: "typed_ledger_fixtures (10) constructs a typed message_board Identity::Session{SessionRef} (23-33) and Refusal variants for RequestLedgerEntry; typed construction only, no conversion or serde",
    },
    {
      file: `${R}lib.rs`,
      disposition: { kind: "none" },
      note: "module declarations under feature client and re-exports (3-151); no definitions",
    },
    {
      file: `${R}mail_status_scenarios.rs`,
      disposition: {
        kind: "excluded",
        reason: `${FLEET_FRAMES}: hand json! delivery statusFetch request (23-26) and expected answers (37, 39, 41)`,
      },
      note: "run_empty_status_stream_conformance (9) checks empty and refused status cursors",
    },
    {
      file: `${R}mutation_revocation_scenario.rs`,
      disposition: {
        kind: "excluded",
        reason: `${FLEET_FRAMES}: expect_refusal (104) callers hand-build eleven consumer/delivery/lease/replication request and refusal bodies with json! (118-214); revokeMachine admin body (41)`,
      },
      note: "piece (154), page (181) and fleet_request (217) serialize typed fixtures with to_value into those bodies",
    },
    {
      file: `${R}native_board_session.rs`,
      disposition: { kind: "none" },
      note: "NativeBoardSession::request (11) sends typed ClientFrame<BoardConsumerProtocol> and decodes ServerFrame; write (58) and snapshot (84) use typed BoardPayload/FleetRequestResult/Record; central board protocol, typed only",
    },
    {
      file: `${R}normal_access_scenarios.rs`,
      disposition: {
        kind: "excluded",
        reason: `Cloudflare Access edge policy and ${FLEET_FRAMES}: run_shared_access_conformance (14) sends signed fixture assertions, a hand json! revokeMachine body (93) and compares the typed close frame {kind:closed,...credentialRejected} (101-102)`,
      },
      note: "decodes AccessRefusal and fleet_sync_protocol AuthRefusal from HTTP bodies (55, 86, 115)",
    },
    {
      file: `${R}readmission_scenarios.rs`,
      disposition: {
        kind: "excluded",
        reason: `central enrollment admin and ${FLEET_FRAMES}: hand json! enrollMachine (48-53, 128-133), revokeMachine (235) and close frame comparisons (40, 171)`,
      },
      note: "control (244) posts central RevocationControlInput to /admin/v1/conformance/revocation",
    },
    {
      file: `${R}repository_runtime.rs`,
      disposition: { kind: "none" },
      note: "run_repository_round_trip_conformance (58) writes typed FleetRequest RepositoryAttach/Detach and compares message_board RepositoryRef lists; central board protocol, typed only",
    },
    {
      file: `${R}revocation_race_scenarios.rs`,
      disposition: {
        kind: "excluded",
        reason: `central enrollment admin and ${FLEET_FRAMES}: hand json! revokeMachine (41), enrollMachine (58-63, 95-100), closed (182) and Hello/Welcome (185, 194)`,
      },
      note: "receive (201) decodes a frame as serde_json Value; control/observed (147, 166) post RevocationControlInput",
    },
    {
      file: `${R}runtime_control_scenarios.rs`,
      disposition: {
        kind: "excluded",
        reason: `central conformance runtime controls and ${FLEET_FRAMES}: run_runtime_control_conformance (24) reads held dispatch JSON by pointer (95, 122-125) and posts a hand json! copy-call body (219)`,
      },
      note: "send/receive (423, 436) use typed ClientFrame/ServerFrame<BoardConsumerProtocol>; held central.forward-route body checked at 122-125",
    },
    {
      file: `${R}runtime_controls.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "central conformance control wire, not one of the six outside edges: RuntimeControlRequest (8-13), RuntimeControl (72-123), DispatchCallSelector (126-138), DispatchKey (141-146) and the observation enums/structs (158-286)",
      },
      note: "OutboundFrameIdentity (296-319, Deserialize) reads only the outer kind/value.frame of an outgoing central fleet frame",
    },
    {
      file: `${R}runtime_faults.rs`,
      disposition: { kind: "none" },
      note: "RuntimeFaults (81) holds in-memory AnswerFault/ObjectReplyFault/FaultState (29-77) and applies RuntimeControl (258); no serde in this file",
    },
    {
      file: `${R}runtime_faults/dispatch_holds.rs`,
      disposition: { kind: "none" },
      note: "DispatchHolds (38) arms, holds and resumes up to four dispatch barriers (9) and reports DispatchHoldObservation (129); no serde",
    },
    {
      file: `${R}runtime_faults/selected_calls.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "central object invocation selector, not one of the six outside edges: InvocationKind (13-16), FleetInvocation (18-22), FleetSubmitIdentity (24-28), SubmittingSession (30-33), FleetItem/FleetItemIdentity (35-43) and SelectedInvocationKind (45-63, central.* tags) Deserialize a held object call",
      },
      note: "DispatchSelection::matches (66) compares the selected central call kind and peer/item",
    },
    {
      file: `${R}runtime_faults/session_faults.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "central fleet-session fault controls, not one of the six outside edges: SessionFaultControl (8-21), FinalizeFaultObservation (23-39), PublicationFaultObservation (41-57), SessionFaultObservation (59-64)",
      },
      note: "RuntimeFaults session methods (73-155) arm and consume one-shot faults",
    },
    {
      file: `${R}runtime_faults/signal.rs`,
      disposition: { kind: "none" },
      note: "Signal (15) is a one-shot waker notification (17, 28); no message shape",
    },
    {
      file: `${R}scenario_assertions.rs`,
      disposition: { kind: "none" },
      note: "check (3) and equal (10) assertion helpers; no message shape",
    },
    {
      file: `${R}scenario_clock_origin.rs`,
      disposition: { kind: "none" },
      note: "logical_scenario_origin (5) returns a fixed RegistryTime; no message shape",
    },
    {
      file: `${R}sender_revocation_scenario.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "run_sender_revocation_conformance (13) hand-builds the board mail recipient {peer, address:{endpoint:{serviceId,endpointId:codex-local},sessionId}} with json! (54-56) and decodes it by from_value into BoardDeliveryTypes::Address = message_board::SessionRef; inventory row. Also hand json! MailSender (53), MessageBody (57-58), mailSubmit and statusFetch frames (65-68, 78-81): central board mail, not one of the six outside edges",
    },
    {
      file: `${R}session_replacement_scenarios.rs`,
      disposition: {
        kind: "excluded",
        reason: `${FLEET_FRAMES}: hand json! connectionReplaced close (41), Hello (51-53) and Welcome (55)`,
      },
      note: "decodes AccessRefusal from the owner-assertion upgrade refusal (31)",
    },
    {
      file: `${R}signin_fixtures.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "organization sign-in JWS fixtures (OAuth/JWT), not one of the four protocols: published_keys (30), claims (51) and stamp_header (72) hand-build Ed25519 JWKS and fleet-organization-stamp claims/headers with json!",
      },
      note: 'SignInKeys (159) serves the fixture key set over loopback; declared under #[cfg(all(feature = "signin-test-support", not(target_arch = "wasm32")))] at lib.rs:5',
    },
    {
      file: `${R}signin_fixtures/person_requests.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "person token and DPoP proof fixtures (OAuth/JWT), not one of the four protocols: Person::token (86) and headers_with (121) hand-build at+jwt claims and dpop+jwt proofs with json!; create/lookup/records intents and create_body (142-154) are organization-admin request vectors",
      },
      note: "canonical (214) is an RFC 8785 encoder for fixture intents; ProofKey (157) is a P-256 key",
    },
    {
      file: `${R}signin_fixtures/statement_refusals.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "organization statement refusal JWS vectors (OAuth/JWT), not one of the four protocols: statement_refusal_case (41) and further_refusal_case (127) build signed claims/headers with json!",
      },
      note: "StatementRefusalCase (5) carries the refusal name and HTTP status",
    },
    {
      file: `${R}snapshot_merge.rs`,
      disposition: { kind: "none" },
      note: "run_fleet_snapshot_merge_conformance (15) writes typed FleetRequests and reads FleetSnapshot pages; reads the serde text of message_board RecordKind via to_value (75, 79) for comparison only; central board protocol",
    },
    {
      file: `${R}staging_access.rs`,
      disposition: { kind: "none" },
      note: "StagingAccess (26) and StagingInstallation (9) hold staging Access credentials read from environment (72); no serde",
    },
    {
      file: `${R}step_fixture.rs`,
      disposition: { kind: "none" },
      note: "receipt_fixture (11), receipt_with_effects_fixture (44) and typed_receipt_fixture (77) construct typed message_board RequestLedgerEntry/DecisionEffect values; typed only",
    },
    {
      file: `${R}transaction_reopen.rs`,
      disposition: {
        kind: "excluded",
        reason: `${FLEET_FRAMES}: CopyPeer::create (38) posts hand json! copy-probe create (52-54) and fence (123) bodies; snapshot (210) pages ReplicationAnswer`,
      },
      note: "row_piece/piece (290, 316) build fleet_sync_protocol Piece fixtures",
    },
    {
      file: `${R}workerd_process.rs`,
      disposition: { kind: "none" },
      note: "WorkerdProcess (20) spawns the Miniflare harness with JSON-encoded Duration arguments (40-41) and waits for 'Ready on' (63); process lifecycle only",
    },
  ],
  sendSites: [],
};
