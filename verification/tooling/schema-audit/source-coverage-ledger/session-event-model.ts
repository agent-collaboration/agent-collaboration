import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/session-event-model",
  files: [
    {
      file: "crates/session-event-model/src/approval_choice.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "ApprovalEffect (7), ApprovalScope (15), StorageDestination (30, string newtype), ApprovalChoice (74); the Router-side approval vocabulary that the approval option tables convert to and from; SessionApprover (88) wraps a message_board Identity and refuses Identity::Session equal to the requester SessionRef (91-96), a check, not a conversion",
    },
    {
      file: "crates/session-event-model/src/capability_report.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "CapabilityReport (6), QueueCapability, QueueSupport, PromptContentCapabilities (50), ProviderAuthStatus (60); JsonSchema derive",
    },
    {
      file: "crates/session-event-model/src/input_identity.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "InputId (9) string newtype, serde try_from String, UUIDv7 generate (21); JsonSchema derive",
    },
    {
      file: "crates/session-event-model/src/interaction_request.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "OfferedOptionId (13), OfferedOption(s) (57,66), ApprovalSubject (123), OptionsOrigin (143), ApprovalToolCallSubject (151), ApprovalRequest (159), ChoiceOption (173), QuestionField (180), QuestionFields (224), QuestionRequest (298), PendingInteraction (306); ApprovalSubject is repeated with different fields in session_profile_codec/approval_metadata.rs:48",
    },
    {
      file: "crates/session-event-model/src/lib.rs",
      disposition: { kind: "none" },
      note: "module declarations and re-exports, including message_board Identity and SessionRef (20); no shape or conversion",
    },
    {
      file: "crates/session-event-model/src/prompt_content.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "PromptContent (89) with validated newtypes (PromptText, PromptUri, PromptMimeType, PromptData); no serde derive, the wire form is PromptContentWire in session_profile_codec.rs:129; re-exported as CommandContent",
    },
    {
      file: "crates/session-event-model/src/provider_skill.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "SkillView (72), SelectedSkill (82), ProviderSkill (43), SkillInvocationName; ephemeral inventory, no serde",
    },
    {
      file: "crates/session-event-model/src/provider_submission.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "ProviderSubmission (6): content plus skill selections",
    },
    {
      file: "crates/session-event-model/src/question_response.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "QuestionResponse (10) and QuestionAnswerValue (20), serde and JsonSchema",
    },
    {
      file: "crates/session-event-model/src/session_event.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "StopReason (7), LocalCause (18), TurnLostReason (25), InteractionCancelReason (33, nine named reasons incl. eventPublicationFailed 39/54/71) and ProviderSettingFailureReason (98) as string tables with manual JsonSchema impls (86,130), TurnOutcome (143), ToolCallStatus (155), SessionItemKind (164), SessionItem (187, text: Option<String>), SessionEvent (201)",
    },
    {
      file: "crates/session-event-model/src/session_profile_codec.rs",
      disposition: {
        kind: "defines",
        owners: ["router-extension", "claude-extra", "cursor-extra", "router-vocabulary"],
      },
      note: "the versioned Router ACP profile: method table _session/steering, queue/add, queue/list, queue/cancel, state (23-27), ProfileAdvertisement (42), InitializeProfileMetadata (71), PromptContentWire parser/encoder (129-250), steering/queue/state request and result types (253-440), RouterIdentityMetadata (489), choice metadata encode (520, hand-built serde_json::Map with literal effect/scope names) and decode (540, via ChoiceMetadataWire 504); StateNotification hand Serialize/Deserialize to StateNotificationWire (447-485) and ProfileTurn (394, open-string stop_reason/reason), ProfileSessionMetadata (557); Claude authStatus decoder decode_connection_auth_status (572); Cursor plan/todo edges ProviderEdge, CursorTodo, translate_provider_edge (619-699)",
    },
    {
      file: "crates/session-event-model/src/session_profile_codec/approval_metadata.rs",
      disposition: { kind: "defines", owners: ["router-extension"] },
      note: "ApprovalPromptMetadata (9), ApprovalRequestProfileMetadata (19) with from_request copy from ApprovalRequest (33); ApprovalRequestProfileFields.subject (28) is crate::ApprovalSubject; a second, production-unused ApprovalSubject (48) and ToolCallSubject (65), imported only by tests/integration/profile_codec_contract.rs:219",
    },
    {
      file: "crates/session-event-model/src/session_settings.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "SessionSettings (13), ConfigValue (23), ConfigValueState (31, untagged); SettingApplicationIntent (5) has no serde",
    },
    {
      file: "crates/session-event-model/src/session_state.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "InteractionKind (5), PendingInteractions (15, serde try_from nonempty Vec), SessionState (70)",
    },
  ],
  sendSites: [],
};
