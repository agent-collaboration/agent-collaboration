import type { LedgerSection } from "../schema-audit-types.ts";

const R = "crates/collaboration-central-cloudflare/src/";

export const section: LedgerSection = {
  root: "crates/collaboration-central-cloudflare",
  files: [
    {
      file: `${R}access_crypto.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "Cloudflare Access JWT verification (OAuth/JWT), not one of the four protocols: ImportAlgorithm (8-12, Serialize) is the WebCrypto RSASSA-PKCS1-v1_5/SHA-256 import parameter passed to importKey (39)",
      },
      note: "verify_rs256_signature (14) serializes the central AccessJwk to a JS object for WebCrypto",
    },
    {
      file: `${R}access_verification.rs`,
      disposition: { kind: "none" },
      note: "policy (14) reads ACCESS_* env vars into the core AccessPolicy; application (91) builds CentralApplication; InvocationLabels::observe (52) logs CentralObservation; no serde",
    },
    {
      file: `${R}binding_attachment.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "hibernatable central fleet-socket attachment state, not one of the six outside edges: BindingAttachment (19-27) serializes through WireBindingAttachment (43-51) and WireRegistration (52-71) with decimal-text integers",
      },
      note: "From/TryFrom (90, 121) convert fleet_sync_session ConnectionRegistration and SyncTime to and from exact decimal strings; fleet session ids, not Router vocabulary identities",
    },
    {
      file: `${R}cloudflare_transport.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "Durable Object byte transport between central objects, not one of the six outside edges: WireCallFailure (9-24) is the private 409 codec of durable_steps CallFailure (25-52) for https://central.internal/call (74)",
      },
      note: "address_header (59) puts the ObjectAddress JSON in x-central-object-address; store_call (115) posts OwnedStoreCall JSON to https://central.internal/store and decodes StoreAnswer (146)",
    },
    {
      file: `${R}composed_control.rs`,
      disposition: { kind: "none" },
      note: "fetch (13) owner-gates /admin/v1/conformance/runtime, decodes central conformance RuntimeControlRequest (28) and forwards RuntimeControl JSON to the object (36); observation (61) answers RuntimeFaultObservation; central conformance admin, no four-protocol shape",
    },
    {
      file: `${R}conformance_admission.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "owner-only central conformance routes, not one of the six outside edges: Command (16-87, Deserialize tag kind/content value) and AttachmentFault (8-14) select /test/* Fleet object observations and faults forwarded by forward_command (97)",
      },
      note: "forward_state (217) and fleet_mail_custody (238) decode the central ForwardStateRequest and FleetMailCustodyRequest; bodies are JSON of fleet_sync_protocol ConnectionId/PeerId and durable_steps OperationId",
    },
    {
      file: `${R}conformance_bootstrap.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "test-only central bootstrap Durable Object, not one of the six outside edges: RequestBody (18-24), Fixture (25-40), Command (60-79) and Fault (115-119) are Deserialize fixture commands for BootstrapCommand, EnrollMachine and Authenticate",
      },
      note: "BootstrapObject::fetch (183) returns a hand json! {result, replyLossObserved} (243) carrying the central bootstrap/enrollment replies",
    },
    {
      file: `${R}conformance_core_store.rs`,
      disposition: { kind: "none" },
      note: "ControlIo (10) is the bootstrap fixture's fixed clock, UUIDv7 id source and refusing object_call (42); new_host (53) builds the receipt store host; no serde",
    },
    {
      file: `${R}conformance_scope_faults.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "central storage fault wrapper and journal observation, not one of the six outside edges: SessionJournalObservation (142-149, Serialize) carries durable_steps OperationRecord/StepRecord",
      },
      note: "ScopeFaultStore (14) injects CopyUnavailableReason::StorageFull rollback (102) and a lost rollback reply (49); session_barrier (128) writes a raw barrier frame to the central fleet socket",
    },
    {
      file: `${R}driver_scenarios.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "test-support (lib.rs:26 feature) SQLx Durable Object driver scenarios, not one of the four protocols: DriverScenarioObject::fetch (95) answers the scenario list or a hand json! {ok, error} result (113-114)",
      },
      note: "User (178, sqlx::FromRow) is the fixture row type; reset/insert_user (134, 144) run fixture SQL",
    },
    {
      file: `${R}driver_scenarios/batch_scenarios.rs`,
      disposition: { kind: "none" },
      note: "fetch_batch scenarios (9-185) over the users/posts fixture tables; SQLx driver assertions only",
    },
    {
      file: `${R}driver_scenarios/row_scenarios.rs`,
      disposition: { kind: "none" },
      note: "row, bind, constraint and batch scenarios (7-607); UserId/Role/Member (523-538) are sqlx::Type/FromRow fixtures; SQLx driver assertions only",
    },
    {
      file: `${R}driver_scenarios/schema.sql`,
      disposition: { kind: "none" },
      note: "users, posts and numbers fixture tables (4-23) for the driver scenarios; relational columns only",
    },
    {
      file: `${R}driver_scenarios/transaction_scenarios.rs`,
      disposition: { kind: "none" },
      note: "DoConnection::transaction scenarios (7-147); AppError (39) is a local error enum; SQLx driver assertions only",
    },
    {
      file: `${R}http_body.rs`,
      disposition: { kind: "none" },
      note: "complete (14) buffers a Worker request body up to 2097152 bytes; over_limit (25) answers 413; no message shape",
    },
    {
      file: `${R}lib.rs`,
      disposition: { kind: "none" },
      note: "module wiring and the Worker fetch entry main (58) routing conformance paths and the core central_router (105); no serde",
    },
    {
      file: `${R}object_location.rs`,
      disposition: { kind: "none" },
      note: "location (3) maps ObjectAddress to a Durable Object binding and name (CENTRAL, PROJECT_COPIES, FLEET, WORKSPACE_STORE); central object naming",
    },
    {
      file: `${R}object_migrations.rs`,
      disposition: { kind: "none" },
      note: "LazyLock SQLx migrators (9-17) for the central object, workspace store and widget fixture; no message shape",
    },
    {
      file: `${R}object_shell.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "Durable Object shell for central Fleet/Copy runtimes and hibernatable fleet sockets, not one of the six outside edges: ShellActor::fetch (344) dispatches /call, /session and conformance paths; websocket_message (480) feeds central fleet frames to the runtime",
      },
      note: "ObjectClockObservation (136-142, Serialize, test-support+conformance) is {nowMs, ids}; config (185) and delivery_config (189) decode RegistryConfig/DeliveryConfig from env JSON; /session decodes a base64 BindingAttachment header (449-456)",
    },
    {
      file: `${R}object_shell/conformance_routes.rs`,
      disposition: { kind: "none" },
      note: "ShellActor::conformance_fetch (8) serves /test/* central observations and faults: decodes RuntimeControl, SessionFaultControl, FleetMailCustodyRequest and (ConnectionId, RegistryTime) tuples; central conformance only, no four-protocol shape",
    },
    {
      file: `${R}organization_signin_link.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "private central sign-in Worker entry, not one of the six outside edges: private_entry::Addressed (112-116, Deserialize) reads only organizationId to address the CentralObject (142-145)",
      },
      note: "is_signin_path (13) lists the identity_contract and AdminRoute paths; trust_settings (29) reads CENTRAL_SIGN_IN_* env vars into SignInTrustSettings",
    },
    {
      file: `${R}platform.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "CloudflareCentralPlatform (17) implements the central core's CentralPlatform: object transport (55), fleet-session WebSocket upgrade (64), Access JWKS fetch (134) and workspace store calls (178); central fleet and Access key transport, not one of the six outside edges",
      },
      note: "upgrade_session (64) serializes a BindingAttachment (104-112) into the base64 x-central-session-binding header for the Fleet object",
    },
    {
      file: `${R}reply_deadline.rs`,
      disposition: { kind: "none" },
      note: "ReplyDeadline (19) owns a setTimeout handle via wasm_bindgen and clears it on Drop (52); no message shape",
    },
    {
      file: `${R}shared_object_store.rs`,
      disposition: { kind: "none" },
      note: "CloudflareStorageHost (51) owns the Durable Object store, alarm and hibernated sockets; send_frame (316) writes opaque central fleet-session frames and close_connection (343) serializes fleet_sync_protocol CloseReason; central fleet socket, not one of the six outside edges",
    },
    {
      file: `${R}shared_object_store/session_capabilities.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "conformance (shared_object_store.rs:36 feature) observations of central fleet-socket attachments and alarms, not one of the six outside edges: SessionConnectionObservation (11-19), AttachmentCodecObservation (33-43), SessionAlarmObservation (45-56) are Serialize",
      },
      note: "conformance_attachment_fault (72) writes a hand json! {invalid:attachment} attachment (84)",
    },
    {
      file: `${R}sign_in_crypto.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "sign-in JWT/DPoP signature verification (OAuth/JWT), not one of the four protocols: EdAlgorithm (34-37) and EcAlgorithm (89-97) are Serialize WebCrypto algorithm parameters",
      },
      note: "WorkerEd25519 (16) and WorkerEs256 (73) import keys and verify via SubtleCrypto",
    },
    {
      file: `${R}sign_in_trust.rs`,
      disposition: { kind: "none" },
      note: "CloudflareSignInTrust (19) composes the core's statement/person verifiers; WorkerSignInKeys::fetch_issuer_keys (84) returns the raw issuer JWKS body; sign-in trust, no message shape",
    },
    {
      file: `${R}signin_launcher_controls.rs`,
      disposition: {
        kind: "excluded",
        reason:
          'test-support fixture clock for the sign-in launcher (declared under #[cfg(feature = "test-support")] at workspace_central_object.rs:349), not one of the four protocols: ClockSample (7-11, Deserialize) is {milliseconds}',
      },
      note: "observe (43) answers the core SignInObservation for an organization",
    },
    {
      file: `${R}statement_store.rs`,
      disposition: { kind: "none" },
      note: "run_batch (206), read (233) and migrate (251) execute OwnedBatch/OwnedStatement on Durable Object SQL storage and map SQL values (75) and constraint kinds (49); storage only",
    },
    {
      file: `${R}storage_cases.rs`,
      disposition: { kind: "none" },
      note: "test-support route (21) forwarding /test-support/storage/*, /call-store/* and /driver/* to case objects; decodes CaseStoreCall (69); storage conformance only",
    },
    {
      file: `${R}storage_cases/case_object.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "test-support storage-boundary case Durable Object, not one of the four protocols: MintOrdered (29-35), PauseTarget (37-42), WaiterTarget (44-49), HeldRequest (51-56), AlarmRollback (58-62) Deserialize and SeamState (64-69) Serialize",
      },
      note: "dispatch (207) answers hand json! case results {opened}, {refused}, {read,outcome}, {minted,outcome}, {alarmMs}",
    },
    {
      file: `${R}storage_cases/case_steps.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "test-support storage case step vocabulary, not one of the four protocols: CaseMigrator (13-18), CaseStep (29-45), CaseAlarm (47-52), CaseBatch (54-59), CaseRead (91-98), CaseStoreCall (111-117), CasePausePoint (119-131)",
      },
      note: "CasePausePoint <-> core PausePoint mappings (133, 148) are storage pause seams, not Router vocabulary",
    },
    {
      file: `${R}storage_measurement.rs`,
      disposition: { kind: "none" },
      note: "storage_bytes (4) reads databaseSize as f64 and checks it is an exact safe integer (8); no message shape",
    },
    {
      file: `${R}store_wire.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "owned central_storage wire to the workspace-store Durable Object, not one of the six outside edges: OwnedStatement (13-18), OwnedGuardedStatement (20-25), OwnedAlarmUpdate (27-32), OwnedBatch (34-39), OwnedStoreCall (41-47)",
      },
      note: "From impls (52-95) copy central_storage Statement/StatementBatch/StoreCall with sql as an owned String",
    },
    {
      file: `${R}worker_clock.rs`,
      disposition: { kind: "none" },
      note: "CloudflareClock (6), ObjectClock (16) and WorkspaceClock (46) read platform time once, optionally pinned by conformance faults or a held fixture time; no message shape",
    },
    {
      file: `${R}workspace_cases.rs`,
      disposition: {
        kind: "excluded",
        reason:
          'test-support controls of the central CentralObject (declared under #[cfg(feature = "test-support")] at workspace_central_object.rs:345), not one of the four protocols: Point (22-33), Pause (48-53), Wait (54-59), Store (60-65), AlarmReceipt (356-364), AlarmReceiptResult (366-385)',
      },
      note: "control (103) decodes workspace_model SessionAddress (140) and MachineLoginControl (109) and passes them typed to the runtime; raw_control (387) runs fixture SQL",
    },
    {
      file: `${R}workspace_central_object.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "org-scoped central workspace Durable Object, not one of the six outside edges: CentralRequest (94-101) and CentralMailRequest (102-109) are Serialize/Deserialize {organizationId, caller, credentialHash, request} wrappers of workspace_model WorkspaceRequest and central_mail CentralMailRequest",
      },
      note: "CentralObject::fetch (241) routes sign-in paths to organization_signin_router (292), /mail (297) and requests (310); CentralStatements (24) and WorkspaceTransport (65) carry central_storage calls",
    },
    {
      file: `${R}workspace_store_object.rs`,
      disposition: { kind: "none" },
      note: "WorkspaceStoreObject::fetch (87) checks x-central-object-address (23-47), decodes OwnedStoreCall (103) and answers StoreAnswer; central storage wire defined in store_wire.rs",
    },
  ],
  sendSites: [],
};
