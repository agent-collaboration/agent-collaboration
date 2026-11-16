import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/collaboration-central-tokio",
  files: [
    {
      file: "crates/collaboration-central-tokio/src/access_crypto.rs",
      disposition: { kind: "none" },
      note: "verify_rs256_signature (7) decodes an AccessJwk's base64url n/e and checks an RS256 signature with ring; Cloudflare Access JWT crypto, no message shape",
    },
    {
      file: "crates/collaboration-central-tokio/src/conformance_board_import.rs",
      disposition: { kind: "none" },
      note: "NativeObjects::import_board_snapshot (4) passes typed central ConformanceBoardState and fleet_sync_protocol SnapshotPage/SnapshotManifest to the Fleet runtime; no serde, no message shape",
    },
    {
      file: "crates/collaboration-central-tokio/src/conformance_control.rs",
      disposition: {
        kind: "excluded",
        reason:
          "owner-authenticated central conformance admin routes, not one of the six outside edges: mount (20) adds /admin/v1/conformance/runtime, FLEET_MAIL_CUSTODY_PATH, /__conformance/admission and FORWARD_STATE_PATH axum routes",
      },
      note: "AdmissionObservation (79-90, Deserialize, tag kind/content value) is the conformance admission observation request; handlers decode central RuntimeControlRequest (122), FleetMailCustodyRequest (52), ForwardStateRequest (141) and answer central observations as JSON",
    },
    {
      file: "crates/collaboration-central-tokio/src/conformance_store.rs",
      disposition: { kind: "none" },
      note: "ConformanceStore (11) wraps the durable_steps AtomicStore to inject a late CopyUnavailableReason::StorageFull rollback (98) after SQLite total_changes (123); storage fault plumbing, no message shape",
    },
    {
      file: "crates/collaboration-central-tokio/src/lib.rs",
      disposition: { kind: "none" },
      note: "module wiring and TokioStorageHost (49): LegacyObjectHost send_frame (195) writes opaque central fleet-session frames to the socket and, under conformance, peeks OutboundFrameIdentity (201) only to drop a selected answer; central fleet socket, not one of the six outside edges",
    },
    {
      file: "crates/collaboration-central-tokio/src/native_logging.rs",
      disposition: { kind: "none" },
      note: "observe (3) logs CentralObservation AccessCheck and OrganizationStatementRefused through tracing; no message shape",
    },
    {
      file: "crates/collaboration-central-tokio/src/native_objects.rs",
      disposition: {
        kind: "excluded",
        reason:
          "native central object host for Fleet/Copy runtimes and the central fleet WebSocket, not one of the six outside edges: serve_socket (467) relays fleet-session frames and serializes fleet_sync_protocol CloseReason into the close frame (501)",
      },
      note: "SessionConnectionSnapshot (13-19, Serialize, conformance) is the central admission observation {connection, peer, registration}; SocketOwnerSnapshot (6-11) has no serde; call_store (545) carries central_storage StoreCall/StoreReply",
    },
    {
      file: "crates/collaboration-central-tokio/src/native_objects/alarm_owner.rs",
      disposition: { kind: "none" },
      note: "run/drive (10, 37) redeliver a failed central object alarm with doubling retry delay on TokioWakeTimer; no message shape",
    },
    {
      file: "crates/collaboration-central-tokio/src/native_objects/health_read.rs",
      disposition: { kind: "none" },
      note: "read_health (22) sums the Copy's SQLite file and WAL lengths for its Health call; HealthFileSample (11) is a conformance-only Rust enum without serde",
    },
    {
      file: "crates/collaboration-central-tokio/src/native_server.rs",
      disposition: { kind: "none" },
      note: "TokioServer (36) binds the TCP listener and serves the central core's central_router (167) and optional JWKS router; TokioSettings (12) has no serde",
    },
    {
      file: "crates/collaboration-central-tokio/src/object_clock.rs",
      disposition: { kind: "none" },
      note: "SharedClock (7) returns the source clock or a conformance-pinned RegistryTime (35); no message shape",
    },
    {
      file: "crates/collaboration-central-tokio/src/object_keys.rs",
      disposition: { kind: "none" },
      note: "address_key/address_from_key (4, 14) map ObjectAddress Fleet/Copy to file stems 'fleet'/'copy-<id>'; central object storage names, not a Router identity",
    },
    {
      file: "crates/collaboration-central-tokio/src/object_storage.rs",
      disposition: { kind: "none" },
      note: "ObjectStorage (31) opens one object's SQLite file, runs central_storage StatementBatch/Statement and keeps the committed alarm (40); StoreMigrations (14) holds SQLx migrators; no serde",
    },
    {
      file: "crates/collaboration-central-tokio/src/organization_signin_link.rs",
      disposition: { kind: "none" },
      note: "OrganizationSignInLink::start (30) binds an owner-only 0600 Unix socket and serves the core's organization_signin_router (41); no serde in this file",
    },
    {
      file: "crates/collaboration-central-tokio/src/platform.rs",
      disposition: {
        kind: "excluded",
        reason:
          "TokioCentralPlatform (31) implements the central core's CentralPlatform: central object transport (321), workspace store calls (313), fleet-session WebSocket upgrade (329) and Cloudflare Access JWKS fetch (353); central board/fleet and OAuth-style key transport, not one of the six outside edges",
      },
      note: "fetch_access_keys (353) returns the raw JWKS body bytes as RawJwks without decoding; conformance passthroughs (43-301) return central observation types",
    },
    {
      file: "crates/collaboration-central-tokio/src/self_hosted_issuer.rs",
      disposition: {
        kind: "excluded",
        reason:
          "self-hosted Access JWT issuance (OAuth/JWT), not one of the four protocols: AssertionHeader (128-133) and AssertionClaims (134-147) are Serialize JWT header/claims signed RS256 by mint (68)",
      },
      note: "jwks (54) builds the central AccessJwks/AccessJwk public key set",
    },
    {
      file: "crates/collaboration-central-tokio/src/sign_in_crypto.rs",
      disposition: { kind: "none" },
      note: "NativeEd25519 (11) and NativeEs256 (34) verify sign-in Ed25519 and ES256 (ProofJwk x/y) signatures with ring; JWT/DPoP crypto, no message shape",
    },
    {
      file: "crates/collaboration-central-tokio/src/sign_in_trust.rs",
      disposition: { kind: "none" },
      note: "NativeSignInTrust (18) composes the core's statement and person-request verifiers; NativeSignInKeys::fetch_issuer_keys (89) returns the raw issuer JWKS body; sign-in trust, no message shape",
    },
    {
      file: "crates/collaboration-central-tokio/src/socket_host.rs",
      disposition: { kind: "none" },
      note: "TokioSockets (25) keeps per-connection fleet_sync_session ConnectionBinding/ConnectionRegistration and an mpsc of SocketCommand Frame/Close (11); central fleet socket state, no serde",
    },
    {
      file: "crates/collaboration-central-tokio/src/sqlite_object_store.rs",
      disposition: { kind: "none" },
      note: "SqliteObjectStore (37) runs central_storage batches in BEGIN IMMEDIATE (75), binds/decodes SqlValue (135, 190) and maps SQLite error kinds to ConstraintKind (250); storage only",
    },
    {
      file: "crates/collaboration-central-tokio/src/startup_controls.rs",
      disposition: { kind: "none" },
      note: "PreparedRuntimeControls::prepare (11) accepts only arming RuntimeControl variants and arms per-object RuntimeFaults; typed conformance fault state, no serde",
    },
    {
      file: "crates/collaboration-central-tokio/src/test_support.rs",
      disposition: { kind: "none" },
      note: 'declares pub mod mail_relay_transport (2); the module is gated by #[cfg(feature = "test-support")] at lib.rs:3',
    },
    {
      file: "crates/collaboration-central-tokio/src/test_support/mail_relay_transport.rs",
      disposition: {
        kind: "excluded",
        reason:
          "test-support (lib.rs:3 feature) client of the central fleet-session mail relay, not one of the six outside edges: operation (10) sends a hand json! {kind:request,value:{frame,body}} fleet frame (14) and decodes ServerFrame<BoardConsumerProtocol> (25)",
      },
      note: "relay_request (46) hand-builds the central delivery body {kind:delivery,value:{kind,value:CallFields{call:{operation,step},..}}} (48-57); fleet/board protocol shapes only",
    },
    {
      file: "crates/collaboration-central-tokio/src/workspace_alarm_owner.rs",
      disposition: { kind: "none" },
      note: "WorkspaceAlarmOwner drive (41) sleeps on the injected RuntimeClock until the workspace runtime's committed alarm and retries with backoff; no message shape",
    },
    {
      file: "crates/collaboration-central-tokio/src/workspace_host.rs",
      disposition: { kind: "none" },
      note: "TokioWorkspaceHost (86) opens per-organization WorkspaceRuntime stores and forwards typed workspace_model WorkspaceRequest (195) and CentralMailRequest (205) plus sign-in identity_contract calls (254-317); central workspace design types passed through, no serde or conversion here",
    },
  ],
  sendSites: [],
};
