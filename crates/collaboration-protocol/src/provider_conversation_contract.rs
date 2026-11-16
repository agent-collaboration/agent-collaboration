//! Provider-neutral external ACP conversation and operation contracts.
use crate::{
    CodexGeneration, EndpointRef, MessageContent, MessageText, NonEmptyText, ObservationTimestamp,
    PositiveSeconds, ProviderIdentity, RouterAccess, SessionRef,
};
use agent_automation::OperationId;
use fleet_identity_protocol::{RemoteLaunchAccess, RemoteLaunchPolicy};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, collections::BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    ClaudeCode,
    Cursor,
}

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderRuntimeIdentity {
    pub provider: ProviderKind,
    pub runtime_name: NonEmptyText,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_version: Option<NonEmptyText>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
pub enum ProviderTransport {
    #[serde(rename = "stdioAcp")]
    StdioAcp,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProviderBindingId(String);

impl TryFrom<String> for ProviderBindingId {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if (1..=256).contains(&value.len()) && !value.contains('\0') {
            Ok(Self(value))
        } else {
            Err("provider binding ID requires 1 to 256 UTF-8 bytes without NUL")
        }
    }
}

impl From<ProviderBindingId> for String {
    fn from(value: ProviderBindingId) -> Self {
        value.0
    }
}

impl JsonSchema for ProviderBindingId {
    fn schema_name() -> Cow<'static, str> {
        "ProviderBindingId".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": 256,
            "pattern": "^[^\\u0000]+$",
            "x-maxUtf8Bytes": 256
        })
    }
}

#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, JsonSchema, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum ProviderCapabilityName {
    Create,
    Prompt,
    Load,
    Cancel,
    Permissions,
    CollaborationMcp,
    ExactOperationReconciliation,
    CallerDetach,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderCapabilityStatus {
    Supported,
    Unsupported,
    Unverified,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderCapabilityEvidence {
    Advertised,
    Observed,
    RouterQualified,
    NotAvailable,
}

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderCapability {
    pub name: ProviderCapabilityName,
    pub status: ProviderCapabilityStatus,
    pub evidence: ProviderCapabilityEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<ProviderCapability>", into = "Vec<ProviderCapability>")]
pub struct ProviderCapabilities(Vec<ProviderCapability>);

impl TryFrom<Vec<ProviderCapability>> for ProviderCapabilities {
    type Error = &'static str;

    fn try_from(capabilities: Vec<ProviderCapability>) -> Result<Self, Self::Error> {
        let names = capabilities
            .iter()
            .map(|capability| capability.name)
            .collect::<BTreeSet<_>>();
        if capabilities.is_empty() || capabilities.len() > 32 {
            return Err("provider capabilities require 1 to 32 entries");
        }
        if names.len() != capabilities.len() {
            return Err("provider capability names must be unique");
        }
        Ok(Self(capabilities))
    }
}

impl From<ProviderCapabilities> for Vec<ProviderCapability> {
    fn from(capabilities: ProviderCapabilities) -> Self {
        capabilities.0
    }
}

impl ProviderCapabilities {
    #[must_use]
    pub fn as_slice(&self) -> &[ProviderCapability] {
        &self.0
    }
}

impl JsonSchema for ProviderCapabilities {
    fn schema_name() -> Cow<'static, str> {
        "ProviderCapabilities".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let item_schema = generator.subschema_for::<ProviderCapability>();
        json_schema!({
            "type": "array",
            "minItems": 1,
            "maxItems": 32,
            "items": item_schema
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderBindingIdentity {
    pub endpoint: EndpointRef,
    pub binding_id: ProviderBindingId,
    pub runtime: ProviderRuntimeIdentity,
    pub transport: ProviderTransport,
    pub generation: CodexGeneration,
    pub capabilities: ProviderCapabilities,
}

/// The binding that admitted a conversation operation. External provider identity
/// remains distinct from the Codex ACP listener that owns a native session.
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ConversationBindingIdentity {
    ExternalProvider {
        binding: ProviderBindingIdentity,
    },
    #[serde(rename_all = "camelCase")]
    CodexAcp {
        endpoint: EndpointRef,
        listener_path: NonEmptyText,
        generation: CodexGeneration,
    },
}

impl ConversationBindingIdentity {
    #[must_use]
    pub fn external_provider(&self) -> Option<&ProviderBindingIdentity> {
        match self {
            Self::ExternalProvider { binding } => Some(binding),
            Self::CodexAcp { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProviderWorkingDirectory(String);

impl TryFrom<String> for ProviderWorkingDirectory {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if (1..=4096).contains(&value.len())
            && !value.contains('\0')
            && std::path::Path::new(&value).is_absolute()
        {
            Ok(Self(value))
        } else {
            Err(
                "provider working directory must be an absolute path of 1 to 4096 bytes without NUL",
            )
        }
    }
}

impl From<ProviderWorkingDirectory> for String {
    fn from(value: ProviderWorkingDirectory) -> Self {
        value.0
    }
}

impl JsonSchema for ProviderWorkingDirectory {
    fn schema_name() -> Cow<'static, str> {
        "ProviderWorkingDirectory".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": 4096,
            "pattern": "^/[^\\u0000]*$",
            "x-maxUtf8Bytes": 4096
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderRequestedPolicy {
    pub access: RouterAccess,
}

impl From<ProviderRequestedPolicy> for RemoteLaunchPolicy {
    fn from(policy: ProviderRequestedPolicy) -> Self {
        Self {
            access: match policy.access {
                RouterAccess::WriteRestricted => RemoteLaunchAccess::WriteRestricted,
                RouterAccess::WorkspaceWrite => RemoteLaunchAccess::WorkspaceWrite,
            },
        }
    }
}

impl From<RemoteLaunchPolicy> for ProviderRequestedPolicy {
    fn from(policy: RemoteLaunchPolicy) -> Self {
        Self {
            access: match policy.access {
                RemoteLaunchAccess::WriteRestricted => RouterAccess::WriteRestricted,
                RemoteLaunchAccess::WorkspaceWrite => RouterAccess::WorkspaceWrite,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderSettingsMappingStatus {
    Verified,
    Unverified,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderAuthenticationState {
    Unverified,
    Authenticated,
    AuthenticationRequired,
    AuthenticationFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderPermissionOutcome {
    Allowed,
    Rejected,
    Unanswered,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EffectiveProviderSettings {
    pub requested_policy: ProviderRequestedPolicy,
    pub mapping_status: ProviderSettingsMappingStatus,
    pub authentication: ProviderAuthenticationState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_permission_mode: Option<NonEmptyText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_outcome: Option<ProviderPermissionOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// The permission mode the caller asked for; absent means auto (R3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_mode: Option<String>,
    /// Why `mode`, the agent's own report, differs from the requested mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode_reason: Option<ProviderModeReason>,
    /// What the agent last reported for its fast option; absent when the
    /// Router hasn't observed this session's settings since it started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast: Option<FastState>,
}

/// Why a session's actual permission mode differs from the requested one.
#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderModeReason {
    /// The agent does not offer its auto mode; its ask-first mode is used.
    AutoNotOffered,
    /// The agent accepted auto but put the session in its ask-first mode.
    AgentFellBack,
    /// The Router has no auto realization for this agent; its mode is unchanged.
    AutoNotRealized,
    /// The agent's own auto refuses without asking and escalation is not
    /// proven, so its ask-first mode stands in.
    EscalationUnproven,
    /// Setting the auto realization failed; the agent kept its own mode.
    AutoNotApplied,
}

/// A Codex session's permission mode at creation (R3): the mode asked for, the
/// mode native read back, and why they differ. Carried on the create result's
/// `_meta.codexRouter.permissionMode` and kept by the Host for show.
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodexPermissionModeReport {
    /// The Router access the session was created with.
    pub access: RouterAccess,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_mode: Option<String>,
    /// The mode native read back; absent when native's report was missing or
    /// contradicted the requested access.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<ProviderModeReason>,
}

impl CodexPermissionModeReport {
    /// The report as the effective settings start and show return.
    #[must_use]
    pub fn effective_settings(&self) -> EffectiveProviderSettings {
        EffectiveProviderSettings {
            requested_policy: ProviderRequestedPolicy {
                access: self.access,
            },
            mapping_status: if self.reason.is_none() {
                ProviderSettingsMappingStatus::Verified
            } else {
                ProviderSettingsMappingStatus::Unverified
            },
            authentication: ProviderAuthenticationState::Unverified,
            provider_permission_mode: None,
            permission_outcome: None,
            mode: self.actual.clone(),
            model: None,
            effort: None,
            requested_mode: self.requested_mode.clone(),
            mode_reason: self.reason,
            // Codex has no fast option and the Router reports none for it (R10).
            fast: None,
        }
    }
}

/// How an agent's auto mode treats an action outside its granted folders (R7).
#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderAutoRealization {
    ClassifierEscalates,
    AsksFirst,
    Unknown,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderRequestedSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Absent means the default, off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast: Option<FastSetting>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderSettingName {
    Mode,
    Model,
    Effort,
    Fast,
}

/// What the caller asks of the agent's fast option. Owner, 2026-11-15:
/// "--fast off is default in ckursor ... unless user provices --fast auto".
/// The Router never turns fast on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FastSetting {
    /// Turn the agent's fast option off and keep it off.
    #[default]
    Off,
    /// Leave the agent's fast option to the agent.
    Auto,
}

/// What the agent last reported for its fast option.
#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FastState {
    Off,
    On,
    /// The agent offers no fast option.
    None,
}

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppliedProviderSetting {
    pub setting: ProviderSettingName,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotAppliedProviderSetting {
    pub setting: ProviderSettingName,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FailedProviderSetting {
    pub setting: ProviderSettingName,
    pub value: String,
    pub reason: session_event_model::ProviderSettingFailureReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InvalidSettingSessionDisposition {
    Closed,
    RemainsCreated,
}

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvalidProviderSetting {
    pub setting: ProviderSettingName,
    pub value: String,
    pub advertised: Vec<String>,
    pub session_disposition: InvalidSettingSessionDisposition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderOperationKind {
    ConversationCreate,
    ConversationLoad,
    ConversationResume,
    ConversationClose,
    ConversationPrompt,
    ConversationCancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderOperationStage {
    Admitted,
    MayHaveDispatched,
    Terminal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConversationOperationFailureStage {
    Discovery,
    Validation,
    Binding,
    Admission,
    Dispatch,
    Settlement,
    Reconciliation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderOperationEffect {
    None,
    Applied,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderReconciliationState {
    Unresolved,
    Confirmed,
    NotReconcilable,
}

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ConversationOperationQueueState {
    RouterQueued,
    NotSubmitted { reason: String },
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationOperationSnapshot {
    pub operation_id: OperationId,
    pub operation: ProviderOperationKind,
    pub binding: ConversationBindingIdentity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<SessionRef>,
    pub stage: ProviderOperationStage,
    pub effect: ProviderOperationEffect,
    pub reconciliation: ProviderReconciliationState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_stop_reason: Option<ProviderPromptStopReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_state: Option<ConversationOperationQueueState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_id: Option<session_event_model::InputId>,
    pub admitted_at: ObservationTimestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_at: Option<ObservationTimestamp>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConversationAdmissionState {
    Admitted,
    Existing,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationOperationSubmission {
    pub admission: ConversationAdmissionState,
    pub operation: ConversationOperationSnapshot,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationCreateRequest {
    pub operation_id: OperationId,
    pub endpoint: EndpointRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<CodexGeneration>,
    pub working_directory: ProviderWorkingDirectory,
    pub created_by: ProviderIdentity,
    pub approver: ProviderIdentity,
    pub requested_policy: ProviderRequestedPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<ProviderRequestedSettings>,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationLoadRequest {
    pub operation_id: OperationId,
    pub target: SessionRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<CodexGeneration>,
    pub working_directory: ProviderWorkingDirectory,
    pub requested_by: ProviderIdentity,
    pub approver: ProviderIdentity,
    pub requested_policy: ProviderRequestedPolicy,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationResumeRequest {
    pub operation_id: OperationId,
    pub target: SessionRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<CodexGeneration>,
    pub working_directory: ProviderWorkingDirectory,
    pub requested_by: ProviderIdentity,
    pub approver: ProviderIdentity,
    pub requested_policy: ProviderRequestedPolicy,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationCloseRequest {
    pub operation_id: OperationId,
    pub target: SessionRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<CodexGeneration>,
    pub requested_by: ProviderIdentity,
    pub approver: ProviderIdentity,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationPromptRequest {
    pub operation_id: OperationId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_id: Option<session_event_model::InputId>,
    pub target: SessionRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<CodexGeneration>,
    pub requested_by: ProviderIdentity,
    pub approver: ProviderIdentity,
    pub prompt: MessageContent,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationCancelRequest {
    pub operation_id: OperationId,
    pub target_operation_id: OperationId,
    pub target: SessionRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<CodexGeneration>,
    pub requested_by: ProviderIdentity,
    pub approver: ProviderIdentity,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationOperationShowRequest {
    pub operation_id: OperationId,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationOperationWaitRequest {
    pub operation_id: OperationId,
    pub timeout_seconds: PositiveSeconds,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationOperationReconcileRequest {
    pub operation_id: OperationId,
}

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderPromptStopReason {
    EndTurn,
    MaxTokens,
    MaxTurnRequests,
    Refusal,
    Cancelled,
    Unknown(String),
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ConversationOperationSettlement {
    Created {
        target: SessionRef,
        effective_settings: EffectiveProviderSettings,
    },
    CreatedWithoutSettings {
        target: SessionRef,
        applied: Vec<AppliedProviderSetting>,
        failed: Vec<FailedProviderSetting>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        not_applied: Vec<NotAppliedProviderSetting>,
    },
    Loaded {
        target: SessionRef,
        effective_settings: EffectiveProviderSettings,
    },
    Resumed {
        target: SessionRef,
        effective_settings: EffectiveProviderSettings,
        history: crate::ProviderHistoryAvailability,
    },
    Closed {
        target: SessionRef,
    },
    PromptCompleted {
        target: SessionRef,
        stop_reason: ProviderPromptStopReason,
        #[serde(skip_serializing_if = "Option::is_none")]
        response: Option<MessageText>,
    },
    CancelRequested {
        target: SessionRef,
        target_operation_id: OperationId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConversationOutputUnavailableReason {
    HostRestarted,
    NotRetained,
    OutputLimitExceeded,
    OutputInvalid,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
// The variant size gap clears the lint threshold only on 64-bit targets.
#[cfg_attr(
    not(target_arch = "wasm32"),
    expect(
        clippy::large_enum_variant,
        reason = "a settled output is the common wait result; the other variants are small markers of this wire shape"
    )
)]
pub enum ConversationOperationWaitOutput {
    Pending,
    Available {
        settlement: ConversationOperationSettlement,
    },
    OutputUnavailable {
        reason: ConversationOutputUnavailableReason,
    },
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationOperationWaitResult {
    pub operation: ConversationOperationSnapshot,
    pub output: ConversationOperationWaitOutput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConversationOperationFailureKind {
    Overloaded,
    InvalidRequest,
    InvalidSetting,
    SettingsUnresolved,
    UnsupportedCapability,
    AuthenticationRequired,
    PermissionRejected,
    Busy,
    NotFound,
    ProviderSessionNotFound,
    StaleGeneration,
    Unavailable,
    ProviderRejected,
    ProtocolViolation,
    OutcomeUnknown,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationOperationFailure {
    pub kind: ConversationOperationFailureKind,
    pub stage: ConversationOperationFailureStage,
    pub effect: ProviderOperationEffect,
    pub message: NonEmptyText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<OperationId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid_setting: Option<InvalidProviderSetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_code: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<SessionRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<EndpointRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability: Option<crate::EndpointAvailability>,
}
