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
      file: "crates/collaboration-service/src/mail_delivery_runtime/installation_directory.rs",
      disposition: { kind: "none" },
      note: "central board protocol between Router and central, not one of the six outside edges: sends BoardConsumerRequest::Read {InstallationPeers (38), InstallationPeer (141)} and matches BoardConsumerAnswer::Read/BoardReadPage (44, 149); fleet PeerId to stored peer string (60, 118, 201) and back (93) is a private storage identity, not a Router vocabulary identity",
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
      file: "crates/collaboration-service/src/mail_delivery_runtime/local_project_admission.rs",
      disposition: { kind: "none" },
      note: "fleet Inbox local admission of a frozen project notification (DeliveryItem<RouterDeliveryTypes>, BoardDeliveryBody::Notification check 41-52): central board/fleet delivery, not one of the four protocols",
    },
    {
      file: "crates/collaboration-service/src/mail_delivery_runtime/poll_handover.rs",
      disposition: { kind: "none" },
      note: "builds a message_board SubscriptionBatch from a mail-protocol NotificationSource::FollowBatch (11-50) and compares stored PushRecord fields (135-143); board and mail vocabulary internal to the subscription owner (SubscriptionBatch is not reachable from an MCP tool), not one of the four protocols",
    },
    {
      file: "crates/collaboration-service/src/mail_delivery_runtime/project_admission.rs",
      disposition: { kind: "none" },
      note: "fleet Outbox admission of project mail with its submission epoch (central board/fleet delivery, not one of the four protocols): Submitter::Project (76), MailReadContext::Project stored (84, 149); fleet ItemId to MailItemId by string (67, 207)",
    },
    {
      file: "crates/collaboration-service/src/mail_delivery_runtime/project_cancellation.rs",
      disposition: { kind: "none" },
      note: "fleet Outbox cancel admission (DeliveryCancel, OutboxAdmission, DeliveryRefusal::CancelAlreadyRequested 13-37): central board/fleet delivery, not one of the four protocols",
    },
    {
      file: "crates/collaboration-service/src/mail_delivery_runtime/pushed_mail.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "Fleet Push acceptance: re-types DeliveryEnvelope<BoardDeliveryTypes> as DeliveryEnvelope<RouterDeliveryTypes> by serde_json to_value/from_value (52) and DeliveryItem back to the board family the same way (37, runtime-test-support); both carry the message_board SessionRef recipient and MailSender. The fleet push itself is central protocol",
    },
    {
      file: "crates/collaboration-service/src/mail_delivery_runtime/remote_admission.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "remote direct send: collaboration_protocol ServiceId of the target SessionRef to message_board ServiceId through String (47); builds typed MessageSendResult::Queued/QueuedMessageSend (116); fleet Outbox admission is central protocol",
    },
    {
      file: "crates/collaboration-service/src/mail_delivery_runtime/remote_calls.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "SDK Submit/Report calls onto the central machine session (BoardConsumerRequest::mail_submit 158, DeliveryRequest::Settle 226, ::Report 261: central protocol, not an outside edge); re-types DeliveryEnvelope<RouterDeliveryTypes> as DeliveryEnvelope<BoardDeliveryTypes> by to_value/from_value (156); decode_call/encode_answer (26, 30) are the SDK's own call records",
    },
    {
      file: "crates/collaboration-service/src/mail_delivery_runtime/remote_status.rs",
      disposition: { kind: "none" },
      note: "central fleet status protocol: DeliveryRequest::StatusFetch (73) and ::StatusAck (110), StatusPublication (132), status_call OperationId is a durable_steps call id (13); not one of the six outside edges",
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
      file: "crates/collaboration-service/src/notification_publisher.rs",
      disposition: { kind: "none" },
      note: "project notification work queue over the native project store and fleet writer lease (fleet_sync_lease WriterState 41-47); IntentRecipientEnrollment built from a resolved PeerId (190); central board/fleet only",
    },
    {
      file: "crates/collaboration-service/src/notification_publisher/cancel_expansions.rs",
      disposition: { kind: "none" },
      note: "freezes a fleet DeliveryCancel<BoardDeliveryTypes> with CancelBody (51-63) into message_board_storage MailCancelExpansion: central board/fleet delivery, not one of the four protocols",
    },
    {
      file: "crates/collaboration-service/src/notification_publisher/frontier_coverage.rs",
      disposition: { kind: "none" },
      note: "reads frozen BoardDeliveryBody::Notification / NotificationSource::FollowBatch (20-34, 90-104) and message_board feed records to judge coverage; board/mail vocabulary only",
    },
    {
      file: "crates/collaboration-service/src/notification_publisher/generation_coverage.rs",
      disposition: { kind: "none" },
      note: "transient GenerationPreventedFollow (10, not serializable) over fleet DeliveryItemRef and MailCancelGroup records: central board/fleet only",
    },
    {
      file: "crates/collaboration-service/src/notification_publisher/handoff_retirement.rs",
      disposition: { kind: "none" },
      note: "retires acknowledged MailHandoffItem prefixes (76-112) through the mail runtime; board/fleet only",
    },
    {
      file: "crates/collaboration-service/src/notification_publisher/handoff_scan.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "publishes frozen handoffs: re-types DeliveryItem<BoardDeliveryTypes> as DeliveryItem<RouterDeliveryTypes> by to_value/from_value (96), carrying the message_board SessionRef recipient",
    },
    {
      file: "crates/collaboration-service/src/notification_publisher/intent_handoffs.rs",
      disposition: { kind: "none" },
      note: "NotifyIntent claim traversal over message_board IntentState/IntentClaim (19-57) and IntentHandoffInput (159, 192); board vocabulary internal to project admission",
    },
    {
      file: "crates/collaboration-service/src/notification_publisher/intent_scan.rs",
      disposition: { kind: "none" },
      note: "scans NotifyIntent records by feed cursor (61-190); message_board storage only",
    },
    {
      file: "crates/collaboration-service/src/notification_publisher/local_cancellation.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "before_local_follow_injection converts collaboration_protocol SessionRef to message_board SessionRef with try_into (17) and compares it with the frozen recipient (57); persist_local_cancel re-types DeliveryCancel<BoardDeliveryTypes> as <RouterDeliveryTypes> by to_value/from_value (199)",
    },
    {
      file: "crates/collaboration-service/src/notification_publisher/remote_cancellation.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "remote cancel expansion: re-types DeliveryCancel<BoardDeliveryTypes> as <RouterDeliveryTypes> by to_value/from_value (101); fleet DeliveryRefusal handling (114-123) is central protocol",
    },
    {
      file: "crates/collaboration-service/src/notification_publisher/settled_frontier.rs",
      disposition: { kind: "none" },
      note: "transient SettledMailPrefix/MailFrontierContext (14, 22) and FollowCompletion over MailHandoff pages; board/fleet only",
    },
    {
      file: "crates/collaboration-service/src/operation_receipt_projection.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: 'projects stored automation operations into the MCP OperationSnapshot: method string to AutomationOperationMethod via Value::String (13) and method.as_str() re-decoded into each family\'s method enum (93-161); OperationFailure built from ScheduleFailure/ConfigurationFailure with typed kind/stage/nextAction flattened to strings by token (50-87, 169), stage literal "configuration" (77), failure.message read by hand from stored JSON (19)',
    },
    {
      file: "crates/collaboration-service/src/project_admission/archive_observation.rs",
      disposition: { kind: "none" },
      note: "appends a message_board Fact LifecycleFact::ArchiveSeen (74-91) under the project lock; board records only",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/activation_boundaries.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:23 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/activity_boundaries.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:7 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/archived_membership.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:25 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/covered_watches.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:9 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/flight_regressions.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:27 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/intent_handoff.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:11 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/membership_completion.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:13 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/membership_corruption.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:29 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/membership_drain.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:31 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/membership_lifecycle.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:15 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/membership_renewal.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:17 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/participant_corruption.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:33 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/participant_delivery.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:35 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/participant_gates.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:37 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/participant_intervals.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:39 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/participant_lifecycle.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:41 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/post_cooldown.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:43 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/project_coverage.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:45 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/project_notice_boundaries.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:47 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/project_watch_fidelity.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:20 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/reactivation_regressions.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:49 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/read_agreement.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:51 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
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
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/window_notices.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:53 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/window_recovery.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:55 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/window_release.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:57 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/window_selection_fences.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:59 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/window_settlement_storage.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:63 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/window_settlement.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:61 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/behavior_tests/window_timing.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at project_admission/behavior_tests.rs:65 (#[path]); behavior_tests is declared under #[cfg(test)] at project_admission/mod.rs:15",
      },
      note: "unit-test module, not product code",
    },
    {
      file: "crates/collaboration-service/src/project_admission/follow_admission.rs",
      disposition: { kind: "none" },
      note: "freezes a FollowBatch BoardDeliveryBody::Notification into DeliveryItem<BoardDeliveryTypes> (83-120); transient FollowBatchProposal (18); board/mail/fleet only",
    },
    {
      file: "crates/collaboration-service/src/project_admission/follow_coverage_validation.rs",
      disposition: { kind: "none" },
      note: "validates GenerationPreventedFollow against locked MailHandoff records (10-76); board/fleet only",
    },
    {
      file: "crates/collaboration-service/src/project_admission/intent_admission.rs",
      disposition: { kind: "none" },
      note: "Serialize IntentRequest (18) / IntentOperation (30) only feed the board request-ledger canonical hash (99-107); message_board request ledger and NotifyIntent transitions; board protocol internal, not one of the four",
    },
    {
      file: "crates/collaboration-service/src/project_admission/intent_handoff.rs",
      disposition: { kind: "none" },
      note: "Serialize IntentRecipientEnrollment (13) / IntentHandoffInput (19), hashed inside IntentOperation; freezes an Intent notification as DeliveryItem<BoardDeliveryTypes> (77); board/fleet only",
    },
    {
      file: "crates/collaboration-service/src/project_admission/intent_re_resolution.rs",
      disposition: { kind: "none" },
      note: "re-resolves NotifyIntent recipients against role assignments (16-141); message_board records only",
    },
    {
      file: "crates/collaboration-service/src/project_admission/mail_frontier_admission.rs",
      disposition: { kind: "none" },
      note: "advances the board mail settled frontier under the project lock (6-28); no message shape",
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
    {
      file: "crates/collaboration-service/src/native_wake_acceptance_fixture.rs",
      line: 18,
      edge: "managed-codex",
      sends: "synthetic initialize result {id, result:{}}",
      owner: { crate: "collaboration-service" },
      origin: "apparatus",
      coverage: {
        kind: "uncovered",
        reason:
          "cfg(test) fake managed Codex socket reply written by test apparatus; no kept complete capture records this fixture connection (it keeps only the parsed incoming requests)",
      },
    },
    {
      file: "crates/collaboration-service/src/native_wake_acceptance_fixture.rs",
      line: 34,
      edge: "managed-codex",
      sends: "synthetic thread/read result {thread:{id, status:{type:idle}}}",
      owner: { crate: "collaboration-service" },
      origin: "apparatus",
      coverage: {
        kind: "uncovered",
        reason:
          "cfg(test) fake managed Codex socket reply written by test apparatus; no kept complete capture records this fixture connection (it keeps only the parsed incoming requests)",
      },
    },
    {
      file: "crates/collaboration-service/src/native_wake_acceptance_fixture.rs",
      line: 56,
      edge: "managed-codex",
      sends: "synthetic turn/start result {turn:{id}}",
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
