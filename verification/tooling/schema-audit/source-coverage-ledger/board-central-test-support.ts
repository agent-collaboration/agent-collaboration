import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/board-central-test-support",
  files: [
    {
      file: "crates/board-central-test-support/src/access_edge.rs",
      disposition: {
        kind: "none",
      },
      note: "central board/fleet protocol test fixture (Router<->central session and central admin HTTP), not one of the four protocols: fixture Access bridge for central /v1/session websocket (upgrade l177, bridge l207); relays central frames, no D0 shapes",
    },
    {
      file: "crates/board-central-test-support/src/claim_grant_drop.rs",
      disposition: {
        kind: "none",
      },
      note: "central board/fleet protocol test fixture (Router<->central session and central admin HTTP), not one of the four protocols: decodes ClientFrame/ServerFrame<BoardConsumerProtocol> (l66, l87) to drop one claim grant",
    },
    {
      file: "crates/board-central-test-support/src/dispatch_cleanup.rs",
      disposition: {
        kind: "none",
      },
      note: "central board/fleet protocol test fixture (Router<->central session and central admin HTTP), not one of the four protocols: releases held central dispatches (l5); no serde",
    },
    {
      file: "crates/board-central-test-support/src/enrollment_edge.rs",
      disposition: {
        kind: "none",
      },
      note: "central board/fleet protocol test fixture (Router<->central session and central admin HTTP), not one of the four protocols: fixture edge for central /admin/v1/machines/enroll (route l87, decode EnrollMachine l207, forward l233) and /v1/session (l86, l287)",
    },
    {
      file: "crates/board-central-test-support/src/enrollment_fixture.rs",
      disposition: {
        kind: "none",
      },
      note: "central board/fleet protocol test fixture (Router<->central session and central admin HTTP), not one of the four protocols: EnrollmentTestCentral l9 real central with no pre-enrolled machine",
    },
    {
      file: "crates/board-central-test-support/src/fixture_shutdown.rs",
      disposition: {
        kind: "none",
      },
      note: "central board/fleet protocol test fixture (Router<->central session and central admin HTTP), not one of the four protocols: cleanup ordering (finish l5); no serde",
    },
    {
      file: "crates/board-central-test-support/src/fleet_observer.rs",
      disposition: {
        kind: "none",
      },
      note: "central board/fleet protocol test fixture (Router<->central session and central admin HTTP), not one of the four protocols: Fleet readback over the fixture's central session (read_fleet l11, fleet_feed l66)",
    },
    {
      file: "crates/board-central-test-support/src/lease_notice_gate.rs",
      disposition: {
        kind: "none",
      },
      note: "central board/fleet protocol test fixture (Router<->central session and central admin HTTP), not one of the four protocols: decodes ServerFrame<BoardConsumerProtocol> (l123) to delay one lease notice",
    },
    {
      file: "crates/board-central-test-support/src/lib.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central board/fleet protocol test fixture (Router<->central session and central admin HTTP), not one of the four protocols: TestCentral l51 and hand-built central admin request {requestId,expectFence,expectAcked} (l180) to central's owner HTTP routes",
      },
      note: "TestCentral l51: real Access-verified central plus machine session for board-side tests; hand-built central admin body l180",
    },
    {
      file: "crates/board-central-test-support/src/machine_enrollment.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central board/fleet protocol test fixture (Router<->central session and central admin HTTP), not one of the four protocols: hand-built central admin enrollMachine body (l57), machineEnrolled reply read (l69-87), enrollment file JSON (l163)",
      },
      note: "EnrolledTestMachine l10: distinct real enrollment and Access bridge per Router; central admin JSON l57-87, enrollment file l163",
    },
    {
      file: "crates/board-central-test-support/src/machine_session.rs",
      disposition: {
        kind: "none",
      },
      note: "central board/fleet protocol test fixture (Router<->central session and central admin HTTP), not one of the four protocols: FleetTestRuntime l19 keeps a central session pump; no serde",
    },
    {
      file: "crates/board-central-test-support/src/mail_wire_observer.rs",
      disposition: {
        kind: "none",
      },
      note: "central board/fleet protocol test fixture (Router<->central session and central admin HTTP), not one of the four protocols: decodes central mail frames ClientFrame/ServerFrame<BoardConsumerProtocol> (l86, l129) into MailWireEvent l16",
    },
    {
      file: "crates/board-central-test-support/src/owner_directory_observation.rs",
      disposition: {
        kind: "excluded",
        reason:
          'central board/fleet protocol test fixture (Router<->central session and central admin HTTP), not one of the four protocols: reads central owner-HTTP Directory entries by JSON field (entry.get("project") l78)',
      },
      note: "reads Directory through central's owner HTTP route without opening live stores",
    },
    {
      file: "crates/board-central-test-support/src/owner_transfers.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central board/fleet protocol test fixture (Router<->central session and central admin HTTP), not one of the four protocols: hand-built central owner transfer bodies move l15 / takeover l33 and TakeoverPreview decode l27",
      },
      note: "owner move/takeover HTTP operations against central while Router sessions serve calls",
    },
    {
      file: "crates/board-central-test-support/src/payload_input_observer.rs",
      disposition: {
        kind: "none",
      },
      note: "central board/fleet protocol test fixture (Router<->central session and central admin HTTP), not one of the four protocols: decodes ServerFrame<BoardConsumerProtocol> (l49) to observe opaque input bodies",
    },
    {
      file: "crates/board-central-test-support/src/piece_integrity_refusal.rs",
      disposition: {
        kind: "none",
      },
      note: "central board/fleet protocol test fixture (Router<->central session and central admin HTTP), not one of the four protocols: decodes ClientFrame (l60) and re-encodes a ServerFrame refusal (l73) at the fixture edge",
    },
    {
      file: "crates/board-central-test-support/src/registration_answer_gate.rs",
      disposition: {
        kind: "none",
      },
      note: "central board/fleet protocol test fixture (Router<->central session and central admin HTTP), not one of the four protocols: decodes ServerFrame<BoardConsumerProtocol> (l72) to hold one registration answer",
    },
  ],
  sendSites: [],
};
