import type { LedgerSection } from "../schema-audit-types.ts";
// Left out by name only: 3 colocated unit-test files (*_test.rs, *_tests.rs):
// mail_push_budget_test.rs (declared lib.rs:62-64), router_delivery_types_tests.rs
// (router_delivery_types.rs:65-67) and session_command_port_test.rs (session_command_port.rs:155-157).
// No file in this crate performs I/O; its ports are traits the Host implements, so it has no send sites.

export const section: LedgerSection = {
  root: "crates/collaboration-contracts",
  files: [
    {
      file: "crates/collaboration-contracts/src/lib.rs",
      disposition: { kind: "none" },
      note: 'Module list and re-exports; scripted_delivery_result is exported only under cfg(any(test, feature = "test-support")) (L21-L22).',
    },
    {
      file: "crates/collaboration-contracts/src/machine_identity.rs",
      disposition: { kind: "none" },
      note: "MachineIdentity L5: Rust-only, no serde; picks the machine label from the Remote Control name, then the OS hostname, then the service id (L23-L38). Inline #[cfg(test)] mod tests at L51.",
    },
    {
      file: "crates/collaboration-contracts/src/native_delivery_result.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "No serde (comment L106-L107): NativeObservedOutcome L9, NativeRefusalCode L20 whose as_str L40 spells the 17 DeliveryRejectionReason wire names and 16 other codes by hand, UnsentCause L82, NativeDeliveryResult L109; reconciled_acceptance L118 and observed L133 map DeliveryOutcome to and from NativeObservedOutcome. Inline #[cfg(test)] mod tests at L243.",
    },
    {
      file: "crates/collaboration-contracts/src/native_route_claim.rs",
      disposition: { kind: "none" },
      note: "NativeRouteClaim L6: Rust-only wrapper pairing the serde RouteClaim (session_delivery_contract.rs:117) with an internal UnsentCause; adds no wire shape. Inline #[cfg(test)] mod tests at L63.",
    },
    {
      file: "crates/collaboration-contracts/src/provider_conversation_backend.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "Host-owned execution boundary for external ACP conversation operations: ProviderConversationBackend trait L32 over the collaboration-protocol request and failure types; default inspect/settings refusals built at L39 and settings_unavailable L100. No serde of its own.",
    },
    {
      file: "crates/collaboration-contracts/src/router_identity.rs",
      disposition: { kind: "none" },
      note: "RouterIdentity L7: Rust-only validated service id, epoch, installation PeerId and MachineIdentity; no serde. Inline #[cfg(test)] mod tests at L54.",
    },
    {
      file: "crates/collaboration-contracts/src/scheduled_run_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "Scheduled-run port: serde types RunAcceptance L105, RunSubmission L112, SettlementEvidence L129, ScheduleCapability L137, ScheduleSupport L147, NativeTurnRef L154, RunSummarySource L161, RunSettlement L167, StopRequestOutcome L184, RunReconciliation L192; ScheduleDestination L12, SchedulePreparationRequest L40 and the other request/outcome structs are Rust-only (no serde); traits ScheduledRunExecution L214, ScheduledRunRoute L260.",
    },
    {
      file: "crates/collaboration-contracts/src/session_command_port.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "The N4 command port (no serde): CreateSessionCommand L22, PromptSessionCommand L30, SteerSessionCommand L38, QueueInputCommand L47 and SetSessionSettingCommand L80 carry the board SessionRef/SessionEndpointRef (L5); SessionSettingsCommand L15-19 carries only model, mode and effort (Option<String> each); CommandFailure L90 carries the protocol SessionRef (L93, L98); CommandContent re-exports session_event_model::PromptContent (alias, not a copy, L12).",
    },
    {
      file: "crates/collaboration-contracts/src/session_delivery_contract.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: 'Serde: DeliveryPrecondition L90, RouteUnavailableReason L110, RouteClaim L117. Rust-only: layer_zero::PreparedPush L38 and DeliveryRequest L45, RoutePresence L56, TargetPresence L72, LoadPolicy L83, NOT_LOADED_REASON L13 ("notLoaded"); traits SessionMessageDelivery L154, TargetPresenceProbe L173, SessionDeliveryRoute L177.',
    },
    {
      file: "crates/collaboration-contracts/src/session_event_hub.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "The N5 hub trait L54 (no serde): HubEvent L14 (sequence + SessionEvent), SessionEventAttachment L19, SkillViewAttachment L25, HubSessionSummary L31 (board SessionRef and Identity), SessionEventHubError L44.",
    },
  ],
  sendSites: [],
};
