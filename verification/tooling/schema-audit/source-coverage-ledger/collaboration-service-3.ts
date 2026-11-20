import type { LedgerSection } from "../schema-audit-types.ts";

const P = "crates/collaboration-service/src/";
const SD = `${P}subscription_delivery/`;

const testModule = (file: string, declaration: string) => ({
  file: `${SD}tests/${file}`,
  disposition: {
    kind: "excluded" as const,
    reason: `cfg(test) module declared at ${SD}tests.rs:${declaration}; tests.rs is declared #[cfg(test)] mod tests at ${SD}mod.rs:25-26`,
  },
  note: "unit-test proofs of the subscription delivery owner, not product code",
});

const MAIL_SEND_UNCOVERED =
  "no node tap test drives collaboration-service mail delivery; the N10 tap (codex-native-integration validated_native_request_node_tap_test) and N6 taps (acp-client-runtime) record their own crates' requests, not frames triggered from this caller through SessionMessageDelivery";

export const section: LedgerSection = {
  root: "crates/collaboration-service",
  files: [
    {
      file: `${P}provider_session_inventory.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "MCP provider_sessions_list: ProviderInventoryCursor JSON in a base64 cursor string (L16, decode L205, encode L384); view_name table (L37); SessionState to ProviderSessionState (L45); protocol SessionRef to board SessionRef by to_value/from_value (L322); literal claude-local endpoint check (L174)",
    },
    {
      file: `${P}push_record_delivery.rs`,
      disposition: { kind: "none" },
      note: "store_first_and_deliver (L14) hands a typed PushRecordDraft to MailDeliveryRuntime::send_local; PushDeliveryFailure (L5) is an in-process error enum",
    },
    {
      file: `${P}push_record_resolver.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "MachineId::from(ServiceId) for RouterLink (L12, L24) and link machine id compared with String::from(service_id) (L42); line_for builds PushLineInput by hand (L22); make_notice_list copies PushRecord into PushRecordNotice (L63)",
    },
    {
      file: `${P}response_budget.rs`,
      disposition: { kind: "none" },
      note: "ResultByteBudget measures serde_json::to_vec length of any result (L37); defines no shape",
    },
    {
      file: `${P}run_projection.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "snapshot builds MCP RunSnapshot: agent_automation CapturedRunInputs and WorkerOutcome to protocol types by to_value/from_value (L21, L30); Claude peer session id string compare (L82); RunPhase to RunState with literal explanations (L116-126)",
    },
    {
      file: `${P}schedule_activation.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "ExecutionDestination to collaboration_contracts ScheduleDestination (L31-74); OwnedThread SessionRef flattened to BindingAddress service/endpoint/thread strings (L57-62)",
    },
    {
      file: `${P}schedule_preparation.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "MCP schedule_prepare: DestinationPreparation to ScheduleDestination (L42); JSON bytes of (scheduleId, destination) as canonical request (L135); stored failure Value decoded to ScheduleFailure (L174-178); ScheduleFailure built by hand (L342, L371, L398, L477)",
    },
    {
      file: `${P}schedule_projection.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "snapshot converts stored ScheduleDefinition (L11) and imported continuity (L15) to MCP ScheduleSnapshot types by to_value/from_value",
    },
    {
      file: `${SD}direct_message_push.rs`,
      disposition: { kind: "sends", owners: ["router-vocabulary"] },
      note: "deliver_direct_message builds the push line (render_push_line L104), PushId to DeliveryCorrelationId by string (L124) and the DeliveryRequest it dispatches (L142); hand DeliveryReceipt literals for hold (L49) and reject (L77)",
    },
    {
      file: `${SD}held_mail_signals.rs`,
      disposition: { kind: "reads", owners: ["router-vocabulary"] },
      note: "target_loaded_since_position reads lifecycle journal records and matches LifecycleSubject::Thread / LifecycleChange::ThreadStatus (L220-229); native_thread_id and SessionRef.session_id are both SessionId, so no conversion",
    },
    {
      file: `${SD}intent_read_cost_counter.rs`,
      disposition: {
        kind: "excluded",
        reason: `cfg(test) module declared at ${SD}intent_progress_tests.rs:11-12, which is declared #[cfg(test)] at ${SD}subscription_service.rs:4-6`,
      },
      note: "tracing layer counting sqlx statements and record decodes for intent progress tests",
    },
    {
      file: `${SD}mod.rs`,
      disposition: { kind: "none" },
      note: "module declarations and re-exports; #[cfg(test)] mod tests at L25-26",
    },
    {
      file: `${SD}native_mail_attempt.rs`,
      disposition: { kind: "sends", owners: ["router-vocabulary"] },
      note: "dispatch_mail_attempt calls SessionMessageDelivery::deliver (L45), the shared route port that writes turn/start or turn/steer (managed Codex) or ACP prompts; NoRoute rewritten as a holdable NotSubmitted literal (L54-64) and queued reconciliation literals (L93-100)",
    },
    {
      file: `${SD}native_mail_readiness.rs`,
      disposition: { kind: "none" },
      note: "prompts reader reconciliation after native mail outcomes (L8, L54); uses reader_for_target (subscription_service.rs:690)",
    },
    {
      file: `${SD}reader_delivery_owner.rs`,
      disposition: { kind: "sends", owners: ["router-vocabulary"] },
      note: "the reader actor; MailAttempt forwards the native mail runtime's DeliveryRequest to dispatch_mail_attempt (L614); pending wait refusal overwrites BoardError.message with a literal (L411-414)",
    },
    {
      file: `${SD}reader_direct_messages.rs`,
      disposition: { kind: "none" },
      note: "chooses deliver/reject/hold per direct message (L37-43); the request is built and dispatched at direct_message_push.rs:142, recorded there",
    },
    {
      file: `${SD}reader_native_mail.rs`,
      disposition: { kind: "none" },
      note: "resume_native_held_mail checks presence and asks the native mail runtime to release held items (L12-68); no message built here",
    },
    {
      file: `${SD}subscription_clock.rs`,
      disposition: { kind: "none" },
      note: "SubscriptionRuntimeClock adapter (L4); no shape",
    },
    {
      file: `${SD}subscription_push.rs`,
      disposition: { kind: "sends", owners: ["router-vocabulary"] },
      note: "activity/expiry push drafts (L65, L113; scope flattened to a string L133, CLI renew text L136), RouterOriginRef canonical string (L159), push line (L182), PushId to DeliveryCorrelationId (L220), DeliveryRequest dispatch (L231); target_session JSON round trip (L295)",
    },
    {
      file: `${SD}subscription_service.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "service handle and owner registry; reader_for_target turns protocol SessionRef into board Identity::Session by to_value/from_value (L690); attempt_checked_mail refusals are NativeDeliveryResult literals (L381-440)",
    },
    {
      file: `${SD}tests.rs`,
      disposition: {
        kind: "excluded",
        reason: `cfg(test) module declared at ${SD}mod.rs:25-26 (#[cfg(test)] mod tests)`,
      },
      note: "unit-test root of subscription delivery, not product code",
    },
    testModule("activity_scope_proofs.rs", "2"),
    testModule("control_view_proofs.rs", "3"),
    testModule("direct_message_proofs.rs", "4"),
    testModule("f6_held_record_proofs.rs", "5"),
    testModule("failed_pass_backoff_proofs.rs", "6"),
    testModule("flight_proofs.rs", "7"),
  ],
  sendSites: [
    {
      file: `${SD}native_mail_attempt.rs`,
      line: 45,
      edge: "managed-codex",
      sends:
        "push line as turn/start or turn/steer through SessionMessageDelivery::deliver (SessionDeliveryRouter to the Codex app-server delivery route) for DirectMessage, BoardNotification and SubscriptionActivity mail",
      owner: { part: "N10" },
      origin: "product",
      coverage: { kind: "uncovered", reason: MAIL_SEND_UNCOVERED },
    },
    {
      file: `${SD}native_mail_attempt.rs`,
      line: 45,
      edge: "acp-agent",
      sends:
        "push line as an ACP prompt or Router-queued input through SessionMessageDelivery::deliver to the provider ACP delivery route",
      owner: { part: "N6" },
      origin: "product",
      coverage: { kind: "uncovered", reason: MAIL_SEND_UNCOVERED },
    },
    {
      file: `${SD}direct_message_push.rs`,
      line: 142,
      edge: "managed-codex",
      sends: "direct message or board notification push line (caller of dispatch_mail_attempt)",
      owner: { part: "N10" },
      origin: "product",
      coverage: { kind: "uncovered", reason: MAIL_SEND_UNCOVERED },
    },
    {
      file: `${SD}direct_message_push.rs`,
      line: 142,
      edge: "acp-agent",
      sends: "direct message or board notification push line (caller of dispatch_mail_attempt)",
      owner: { part: "N6" },
      origin: "product",
      coverage: { kind: "uncovered", reason: MAIL_SEND_UNCOVERED },
    },
    {
      file: `${SD}subscription_push.rs`,
      line: 231,
      edge: "managed-codex",
      sends:
        "subscription activity or expiry push line, Auto mode, unpinned (caller of dispatch_mail_attempt; expiry triggered from reader_delivery_actions.rs:62)",
      owner: { part: "N10" },
      origin: "product",
      coverage: { kind: "uncovered", reason: MAIL_SEND_UNCOVERED },
    },
    {
      file: `${SD}subscription_push.rs`,
      line: 231,
      edge: "acp-agent",
      sends:
        "subscription activity or expiry push line, Auto mode, unpinned (caller of dispatch_mail_attempt; expiry triggered from reader_delivery_actions.rs:62)",
      owner: { part: "N6" },
      origin: "product",
      coverage: { kind: "uncovered", reason: MAIL_SEND_UNCOVERED },
    },
    {
      file: `${SD}reader_delivery_owner.rs`,
      line: 614,
      edge: "managed-codex",
      sends:
        "native mail runtime DeliveryRequest (attempt_mail_once / attempt_follow_mail_once) forwarded to dispatch_mail_attempt after the frozen-follow cancellation check",
      owner: { part: "N10" },
      origin: "product",
      coverage: { kind: "uncovered", reason: MAIL_SEND_UNCOVERED },
    },
    {
      file: `${SD}reader_delivery_owner.rs`,
      line: 614,
      edge: "acp-agent",
      sends:
        "native mail runtime DeliveryRequest (attempt_mail_once / attempt_follow_mail_once) forwarded to dispatch_mail_attempt after the frozen-follow cancellation check",
      owner: { part: "N6" },
      origin: "product",
      coverage: { kind: "uncovered", reason: MAIL_SEND_UNCOVERED },
    },
  ],
};
