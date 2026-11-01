//! Shared collaboration vocabulary and capability ports; no runtime resource ownership.
mod project_claim;
pub use project_claim::{ProjectClaim, ProjectClaimError, ProjectClaimFuture};
mod project_registration;
pub use project_registration::{
    ProjectRegistration, ProjectRegistrationError, ProjectRegistrationFuture,
    ProjectRegistrationState,
};
mod project_replication_position;
pub use project_replication_position::{
    AcknowledgedMailChangesFuture, ProjectReplicationPosition, ReplicatedPositionFuture,
    UnacknowledgedProjectReplication,
};
mod machine_identity;
pub use machine_identity::MachineIdentity;
mod harness_launch_identity;
mod harness_session_launch;
pub use harness_launch_identity::{
    HarnessDeclaredLaunchInput, HarnessDeclaredSettings, HarnessForkInput, HarnessLaunchIdentity,
    HarnessLaunchIdentityEncoding, HarnessMcpConfigurationIdentity,
};
mod native_delivery_result;
mod provider_conversation_backend;
mod scheduled_run_contract;
mod session_command_port;
pub use harness_session_launch::{
    HarnessLaunchFailure, HarnessLaunchFuture, HarnessLocalSetupRefusal,
    HarnessRegistrationOutcome, HarnessRestoreAdmission, HarnessSessionBinding,
    HarnessSessionLaunch, HarnessSessionLaunchPort, HarnessStartAdmission,
};
pub use harness_session_launch::{
    HarnessSessionSetup, PreparedHarnessSessionStart, RestoredHarnessSession,
};
mod session_event_hub;
#[cfg(any(test, feature = "test-support"))]
pub use native_delivery_result::scripted_delivery_result;
mod native_route_claim;
pub use native_route_claim::NativeRouteClaim;
mod session_delivery_contract;
pub use native_delivery_result::{
    NativeDeliveryResult, NativeObservedOutcome, NativeRefusalCode, UnsentCause,
};
pub use provider_conversation_backend::{
    ProviderConversationBackend, ProviderConversationFuture, ProviderSessionInspectFuture,
    ProviderSettingsFuture,
};
pub use scheduled_run_contract::{
    FreshSessionRequest, NativeTurnRef, PreparationEvidenceSink, PreparedTarget, RunAcceptance,
    RunEvidenceDisposition, RunEvidenceSink, RunObservationContext, RunReconciliation,
    RunSettlement, RunSubmission, RunSummarySource, ScheduleCapability, ScheduleDestination,
    SchedulePreparationFailure, SchedulePreparationOutcome, SchedulePreparationRequest,
    ScheduleSupport, ScheduledRunExecution, ScheduledRunPayload, ScheduledRunRoute,
    ScheduledRunSubmission, SettlementEvidence, StopRequestOutcome,
};
pub use session_command_port::{
    CommandContent, CommandFailure, CommandFuture, CreateSessionCommand, PromptSessionCommand,
    QueueInputCommand, QueuedSessionInput, SessionCommandPort, SessionSettingsCommand,
    SessionSteerOutcome, SessionTargetCommand, SessionTurnHandle, SetSessionSettingCommand,
    SteerSessionCommand,
};
pub use session_delivery_contract::layer_zero;
pub use session_delivery_contract::{
    AttemptEvidenceSink, AttemptReconciliation, AttemptReconciliationContext,
    DeliveryContractError, DeliveryFuture, DeliveryPrecondition, LoadPolicy, NOT_LOADED_REASON,
    RouteClaim, RoutePresence, RouteUnavailableReason, SessionDeliveryRoute,
    SessionMessageDelivery, TargetPresence, TargetPresenceProbe, UnstoredAttemptEvidenceSink,
};
pub use session_event_hub::{
    HubEvent, HubFuture, HubSessionSummary, SessionEventAttachment, SessionEventHub,
    SessionEventHubError,
};
mod router_delivery_types;
pub use router_delivery_types::{
    RouterConsumerProtocol, RouterDeliveryTypes, RouterLocalObservation,
};
#[cfg(test)]
mod mail_fetch_budget_tests;

mod router_identity;
pub use router_identity::{RouterIdentity, RouterIdentityError};
