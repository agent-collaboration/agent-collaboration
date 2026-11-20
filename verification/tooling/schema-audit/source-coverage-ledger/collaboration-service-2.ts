import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/collaboration-service",
  files: [
    {
      file: "crates/collaboration-service/src/mail_delivery_runtime/inbound_wake_probe.rs",
      disposition: { kind: "none" },
      note: "feature test-support entry: test_support_deliver_inbound_wake (8) passes a fleet DeliveryItem<RouterDeliveryTypes> and mail-protocol WakeBody straight to MailConsumer::deliver_wake; permit count probe (18). Defines, converts, sends or reads no shape of the four protocols",
    },
    {
      file: "crates/collaboration-service/src/mail_delivery_runtime/local_admission.rs",
      disposition: { kind: "none" },
      note: "fleet Inbox custody for local direct mail (central board/fleet delivery SDK, not one of the four protocols): map_refusal (306) maps fleet DeliveryRefusal {ItemConflict, MailboxFull, TooOld} to mail-protocol MailRefusal; the MCP-facing step (MailRefusal to MessageFailureKind) is message_operations.rs:386, outside this share. Reads typed MessageSendResult::Local (239, 290); PushId to MailItemId (149)",
    },
    {
      file: "crates/collaboration-service/src/mail_delivery_runtime/local_calls.rs",
      disposition: { kind: "none" },
      note: "durable_steps CallRouter: decodes the SDK's own CallEnvelope target/request (DeliverCall, DeliveryItemRef) with from_value (13-16) and encodes validated_answer (109); private fleet SDK call records, not one of the four protocols. Native delivery is triggered through MailConsumer/reader actors (82, 97, 106); no transport is written here",
    },
    {
      file: "crates/collaboration-service/src/mail_delivery_runtime/local_cancellation.rs",
      disposition: { kind: "none" },
      note: "fleet DeliveryCancel local custody and mail-protocol CancelCause attribution (central board/fleet delivery, not one of the four protocols); fleet ItemId to MailItemId by string (148)",
    },
    {
      file: "crates/collaboration-service/src/mail_delivery_runtime/remote_wakes.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "remote wake: generic JSON round-trip convert (24) between collaboration_mail_protocol RemoteWake* types and collaboration_protocol Wake* MCP request/result types (363-465), SessionRef protocol to board (132) and compared via JSON (371), ServiceId via String (94), WakeFailureReason/LocalMutationState to RemoteWakeRefusal/RemoteWakeMutationState via JSON (493, 497)",
    },
    {
      file: "crates/collaboration-service/src/mail_delivery_runtime/sender_binding.rs",
      disposition: { kind: "none" },
      note: "fleet SenderBinding: mail-protocol MailSender to fleet Submitter (59-87); ProjectId/peer strings to fleet DatabaseId/PeerId; central board/fleet identities, not Router vocabulary identities",
    },
    {
      file: "crates/collaboration-service/src/mail_push_delivery.rs",
      disposition: { kind: "none" },
      note: "recipient push policy: prepares a MailPushPreparation and hands layer_zero::DeliveryRequest to SubscriptionDeliveryService (attempt_follow_mail_once 114, attempt_mail_once 124); the native transport sits behind the SessionMessageDelivery trait, implemented elsewhere; no shape defined or converted here",
    },
    {
      file: "crates/collaboration-service/src/mail_push_delivery_fixture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at mail_push_delivery_tests.rs:3-4 (#[path] mod native), itself declared under #[cfg(test)] at mail_push_delivery.rs:150-152",
      },
      note: "test stand-in SessionMessageDelivery recording layer_zero::DeliveryRequest in memory (97); hand-built message_board SessionRef JSON (415) and record subject JSON (435) in test data; writes no Codex/ACP/MCP frame",
    },
    {
      file: "crates/collaboration-service/src/mail_push_outcome.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "maps Router DeliveryOutcome to fleet LocalOutcome/mail CarrierAcceptance twice (observed_outcome 40, local_follow_outcome 84, the second holding NotSubmitted/Rejected instead of refusing); UnsentCause/NativeRefusalCode to fleet DeliveryRefusal/DeliveryFailureKind (22-31, 62-77)",
    },
    {
      file: "crates/collaboration-service/src/native_claude_catalogue.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "builds MCP provider_sessions_list rows (ProviderSessionSummary::ClaudeCodeBackground 151, ::ClaudeCodeInteractive 182, NativeClaudeInventoryCoverage 204) from claude_code_peer_messaging records; peer_status (213) maps PeerSessionStatus to ClaudeCodeInteractiveStatus, dropping Other's value; SessionRef built from endpoint plus native session id (78, 153, 184)",
    },
    {
      file: "crates/collaboration-service/src/native_loaded_permission_fixture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at native_loaded_permission_tests.rs:13-14 (#[path] mod policy_fixture), declared at native_recorded_permission_tests.rs:575-576, declared under #[cfg(test)] at lib.rs:101-102",
      },
      note: "fake managed Codex app-server reply table (initialize 164, thread/read 166, thread/resume 169, turn/start 181, thread/turns/list 200, turn/steer 203, thread/queue/add 204, reply envelope 216) and hand-built Router run JSON decoded by from_value (CapturedRunInputs 110, CodexAppServer recorded receipt 111); kept as codex-stand-in-schemas inventory row",
    },
    {
      file: "crates/collaboration-service/src/native_wake_acceptance_fixture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at native_delivery_crash_tests.rs:27-28 (#[path] mod native_acceptance), declared at lib.rs:97-98 inside #[cfg(test)] mod wakeup_delivery_sender (lib.rs:86-87)",
      },
      note: "fake managed Codex socket: initialize result (19), thread/read method check (30) and result (35), turn/start method check (47), params.clientUserMessageId read (51), turn/start result (57); kept as codex-stand-in-schemas inventory row",
    },
    {
      file: "crates/collaboration-service/src/operation_receipt_projection.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: 'projects stored automation operations into the MCP OperationSnapshot: method string to AutomationOperationMethod via Value::String (13) and method.as_str() re-decoded into each family\'s method enum (93-161); OperationFailure built from ScheduleFailure/ConfigurationFailure with typed kind/stage/nextAction flattened to strings by token (50-87, 169), stage literal "configuration" (77), failure.message read by hand from stored JSON (19)',
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/subscription_fixture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:2 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
  ],
  sendSites: [
    {
      file: "crates/collaboration-service/src/native_loaded_permission_fixture.rs",
      line: 215,
      edge: "managed-codex",
      sends:
        "synthetic app-server replies {id, result} for initialize, thread/read, thread/resume, turn/start, thread/turns/list, turn/steer and thread/queue/add from the reply table l164-205",
      owner: { crate: "collaboration-service" },
      origin: "apparatus",
      coverage: {
        kind: "uncovered",
        reason:
          "cfg(test) fake managed Codex socket reply written by test apparatus; no kept complete capture records this fixture connection (it keeps only the parsed incoming requests)",
      },
    },
  ],
};
