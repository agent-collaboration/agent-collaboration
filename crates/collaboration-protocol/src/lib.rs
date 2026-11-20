//! Public communication contracts without process, storage or transport ownership.
mod cli_output_contract;
pub use cli_output_contract::{
    ConversationRecord, ConversationTerminalReason, EffortChange, FiniteCommandRecord,
    NativeObservationRecord, ObservationCloseReason,
};
mod operation_failure_contract;
pub use operation_failure_contract::{
    AdapterOperationFailure, OperationEffect, OperationFailureKind,
};
mod conversation_create_outcome;
mod provider_conversation_contract;
mod provider_identity;
mod provider_settings_contract;
pub use conversation_create_outcome::ConversationCreateOutcome;
pub use provider_conversation_contract::{
    AppliedProviderSetting, CodexPermissionModeReport, ConversationAdmissionState,
    ConversationBindingIdentity, ConversationCancelRequest, ConversationCloseRequest,
    ConversationCreateRequest, ConversationLoadRequest, ConversationOperationFailure,
    ConversationOperationFailureKind, ConversationOperationFailureStage,
    ConversationOperationQueueState, ConversationOperationReconcileRequest,
    ConversationOperationSettlement, ConversationOperationShowRequest,
    ConversationOperationSnapshot, ConversationOperationSubmission,
    ConversationOperationWaitOutput, ConversationOperationWaitRequest,
    ConversationOperationWaitResult, ConversationOutputUnavailableReason,
    ConversationPromptRequest, ConversationResumeRequest, EffectiveProviderSettings,
    FailedProviderSetting, FastSetting, FastState, InvalidProviderSetting,
    InvalidSettingSessionDisposition, NotAppliedProviderSetting, ProviderAuthenticationState,
    ProviderAutoRealization, ProviderBindingId, ProviderBindingIdentity, ProviderCapabilities,
    ProviderCapability, ProviderCapabilityEvidence, ProviderCapabilityName,
    ProviderCapabilityStatus, ProviderKind, ProviderModeReason, ProviderOperationEffect,
    ProviderOperationKind, ProviderOperationStage, ProviderPermissionOutcome,
    ProviderPromptStopReason, ProviderReconciliationState, ProviderRequestedPolicy,
    ProviderRequestedSettings, ProviderRuntimeIdentity, ProviderSettingName,
    ProviderSettingsMappingStatus, ProviderTransport, ProviderWorkingDirectory,
};
pub use provider_identity::ProviderIdentity;
pub use provider_settings_contract::{
    ProviderSettingsAcceptRequest, ProviderSettingsFailure, ProviderSettingsFailureKind,
    ProviderSettingsResult, ProviderSettingsSetRequest,
};
mod access_contract;
mod permission_diagnostic;
mod provider_session_inspect;
mod provider_session_list;
mod session_observation_contract;
pub use access_contract::{
    PermissionDifference, RouterAccess, SettingsObservation, SettingsObservationSource,
    SettingsUnavailableReason,
};
pub use permission_diagnostic::{
    PermissionDiagnostic, PermissionDiagnosticKind, PermissionDiagnosticNextAction,
    PermissionDiagnosticStage,
};
pub use provider_session_inspect::{
    ProviderConfigOptionView, ProviderConfigValueView, ProviderHistoryAvailability,
    ProviderInspectFailure, ProviderInspectFailureKind, ProviderSessionInspectRequest,
    ProviderSessionInspectResult, ProviderSettingChoiceView, ProviderSettingsCatalogView,
};
pub use provider_session_list::{
    ClaudeCodeBackgroundOrigin, ClaudeCodeInteractiveOrigin, ClaudeCodeInteractiveStatus,
    HostedProviderOrigin, NativeClaudeInventoryCoverage, NativeClaudePeerObservation,
    NativeClaudeSessionConflict, ProviderSessionListParams, ProviderSessionListResult,
    ProviderSessionState, ProviderSessionSummary,
};
mod native_claude_inventory;
pub use native_claude_inventory::{
    NativeClaudeBackgroundState, NativeClaudeConflictReason, NativeClaudeInventoryPart,
    NativeClaudeJobHandle, NativeClaudeStateName, NativeInventoryInvalidReason,
    NativeInventorySource, NativeInventoryUnavailableReason,
};
pub use session_observation_contract::{
    BoundedObservationRequest, BoundedObservationResult, OBSERVATION_EVENT_NOTIFICATION,
    ObservationCursor, ObservationEndReason, ObservationEventNotification,
    ProviderObservationEventTooLarge, ProviderObservationEventTooLargeKind,
};
mod backend_generation;
mod delivery_rejection;
mod native_delivery_failure;
pub use native_delivery_failure::{
    NativeDeliveryFailure, NativeDeliveryFailureCause, NativeDeliveryOperation, NativeDeliveryStage,
};
mod delivery_route_evidence;
mod message_content;
mod push_line;
mod session_delivery_outcome;
pub use delivery_rejection::{
    DeliveryNextAction, DeliveryPeerClaim, DeliveryRejection, DeliveryRejectionReason,
};
mod session_delivery_receipt;
pub use delivery_route_evidence::{DeliveryRouteEvidence, ProviderAcpRouteEvidence};
pub use session_delivery_outcome::{
    DeliveryCorrelationId, DeliveryIdentityError, DeliveryOutcome, SessionReachability,
};
pub use session_delivery_receipt::{DeliveryClientReceipt, DeliveryReceipt};
mod native_schema_references;
mod native_session_catalog;
pub use message_content::{
    AcceptedResumeEffect, MessageContent, MessageDelivery, MessageInputKind, MessageRepresentation,
    MessageText, MessageTextError, RenderedMessage, SessionDisplayName, SessionDisplayNameError,
};
pub use native_control_contract::{
    NativeInputDisposition, NativeInputOperation, NativeSendAcceptance, NativeSendParams,
    NativeSendReceipt,
};
pub use native_session_catalog::{
    NativeSessionListParams, NativeSessionListResult, NativeSessionObservation, NativeSessionScope,
    NativeSessionSource, NativeSessionSummary, NativeSessionView,
};
pub use push_line::{
    MAX_MACHINE_LABEL_SCALARS, MAX_PUSH_LINE_BYTES, MachineId, MachineLabel, MachineLabelError,
    ParsedPushLine, ParsedPushLineHeader, PushId, PushIdError, PushKind, RouterLink,
    escape_push_line_field, parse_push_line, parse_push_line_header,
};
mod endpoint_inventory;
pub use endpoint_inventory::EndpointInventory;
mod identity_schemas;
mod installation_peer_identity;
pub use installation_peer_identity::{
    InstallationIdentity, InstallationPeerId, InstallationPeerIdentityError,
};
mod protocol_type_schemas;
pub use protocol_type_schemas::protocol_type_schemas;
mod native_control_contract;
pub use native_control_contract::{
    NativeInspectParams, NativeInspectResult, NativeInterruptKind, NativeInterruptParams,
    NativeInterruptResult, NativeRenameParams, NativeRenameResult,
};
mod endpoint_identity;
pub use backend_generation::{CodexGeneration, GenerationNumber};
pub use endpoint_identity::{
    EndpointId, EndpointRef, IdentityError, SessionId, SessionRef, UuidIdentity,
};
mod message_frame_limit;
pub use message_frame_limit::MAX_CONTROL_FRAME_BYTES;
mod router_executable_relation;
pub use router_executable_relation::{RouterExecutableRelation, router_build_warning};

mod endpoint_description;
pub use endpoint_description::{
    AcpCarrier, ChannelDescription, EndpointAvailability, EndpointDescription, NativeCarrier,
    ObservationTimestamp, SchemaDigest,
};

pub use endpoint_description::NonEmptyText;

mod service_manifest;
pub use service_manifest::{
    ApiSelector, ApiSocketPath, ApiTransport, McpSelector, McpTransport, SERVICE_MANIFEST_VERSION,
    ServiceManifest,
};
mod lifecycle_observation;
pub use lifecycle_observation::{
    BackendStatus, LifecycleChange, LifecycleObservation, LifecycleSubject, NativeActiveFlag,
    NativeThreadStatus, ObservationOrdering, ObservationScope, ObservationSource, TerminalStatus,
    ThreadAddress,
};

mod journal_contract;
pub use journal_contract::{
    AddressListParams, JournalBounds, JournalPage, JournalPosition, JournalReadParams,
    JournalStatus, LifecycleRecord,
};

mod address_book_contract;
pub use address_book_contract::{
    AddressEntry, AddressPage, ArchiveState, CoverageState, CoverageView, Existence,
    LifecycleDisposition, StatusOrdering,
};

mod acp_schema_catalog;
pub use acp_schema_catalog::{
    ACP_SCHEMA_BYTES, ACP_SCHEMA_DIGEST, AcpSchemaCatalog, AcpSchemaError,
};

pub use agent_automation::SummarySourceReference;
pub use agent_automation::{AttemptId, DeliveryId, EventId, PeerWriteEffect};
pub use agent_automation::{ChangeId, RunId, ScheduleId};
pub use agent_automation::{InstructionId, InstructionText, OperationId, RevisionId, WakeupId};
mod positive_seconds;
pub use positive_seconds::{InvalidSeconds, PositiveSeconds};
mod delivery_inspection_contract;
pub use delivery_inspection_contract::{
    CessationEvidence, NativeEffectEvidence, PreparationEffect, SubmissionEffect,
};

// The facade preserves mail identity through every local message boundary.
