//! Resource-backed collaboration execution, provider adapters, durable owners and workers.
mod project_store_paths;
mod project_store_registry;
pub use project_store_registry::{NativeProjectStore, ProjectRegistryError, ProjectStoreRegistry};
mod central_project_reads;
mod project_read_handle;
pub use central_project_reads::{
    CentralReadError, ProjectReadCopyFuture, ProjectReadCopySource, PulledProjectReadCopy,
};
pub use project_read_handle::{ProjectReadHandle, ProjectReadSnapshot};
mod fleet_client;
pub use fleet_client::{FleetClient, FleetClientError, FleetReply};
mod automation_retention_worker;
mod project_claim_store;
pub use automation_retention_worker::AutomationRetentionWorker;
mod endpoint_directory;
pub use endpoint_directory::{EndpointDirectory, EndpointInventoryReader, EndpointSnapshot};
mod native_channel_relay;
pub use native_channel_relay::{
    MAX_NATIVE_MESSAGE_BYTES, connect_native_relay, relay_native_channels,
};
mod native_generation_gate;
pub use native_generation_gate::{NativeAdmission, NativeGenerationGate};
mod native_relay_listener;
mod pending_snapshot_interactions;
mod private_socket_listener;
pub use native_relay_listener::NativeRelayListener;
pub use private_socket_listener::{OwnerOnlySocket, SocketCleanup};
mod codex_conversation_operation_recorder;
mod provider_operation_store;
pub use codex_conversation_operation_recorder::{
    CodexConversationOperationRecorder, UnavailableConversationOperationRecorder,
};
mod provider_session_event_hub;
mod provider_session_record;
pub use provider_operation_store::{
    ProviderOperationAdmission, ProviderOperationAdmissionResult, ProviderOperationRecord,
    ProviderOperationStore, ProviderOperationStoreError,
};
mod delivery_acceptance_effect;
pub mod delivery_route_projection;
pub use provider_session_event_hub::{HubReceiveError, ProviderSessionEventHub, receive_hub_event};
pub use provider_session_record::ProviderSessionRecord;
mod app_server_event_forwarding;
mod app_server_item_metadata;
mod app_server_item_translation;
mod app_server_model_catalog;
pub use app_server_item_translation::{
    ApprovalPresentation, ApprovalReply, HistoricalTurn, InteractionDisplayContext,
    InteractionReplyError, QuestionPresentation, QuestionReply, TranslatedSessionItem,
    group_historical_turns, map_approval_reply, map_question_form_reply,
    map_request_user_input_reply, render_historical_turn, translate_approval_request,
    translate_question_request, translate_session_item,
};
#[cfg(test)]
#[path = "app_server_interaction_translation_tests.rs"]
mod app_server_interaction_translation_tests;
#[cfg(test)]
#[path = "app_server_item_translation_tests.rs"]
mod app_server_item_translation_tests;
mod provider_acp_content_translation;
mod provider_acp_event_projection;
mod provider_acp_interaction;
mod provider_acp_session_route;
mod router_session_app_server;
pub mod schedule_preparation_evidence_sink;
mod scheduled_run_evidence_sink;
mod scheduled_run_router;
pub use app_server_model_catalog::{
    InvalidProviderModelEntry, ProviderModelEntry, render_model_list,
};
pub use router_session_app_server::{
    AppServerConnectionError, RouterSessionAppServerContext, RouterSessionAppServerListener,
    serve_router_session_app_server_connection,
};
mod session_delivery_router;
pub mod stored_delivery_receipt;
pub mod stored_run_receipt;
pub use provider_acp_session_route::ProviderAcpSessionRoute;
pub use session_delivery_router::SessionDeliveryRouter;
mod service_identity_storage;
pub use service_identity_storage::{
    load_installation_peer, load_service_identity, new_service_uuid, read_service_identity,
};
mod manifest_publication;
pub use manifest_publication::ManifestPublication;
mod native_control_backend;
mod session_display_name_cache;
pub use native_control_backend::NativeControlBackend;
pub use session_display_name_cache::SessionDisplayNameCache;
mod codex_app_server_delivery_route;
mod codex_app_server_scheduled_runs;
mod codex_queue_reconciliation;
mod interaction_broker;
mod message_effect_state;
pub use message_effect_state::classify_native_rejection;
mod native_message_dispatch;
mod native_recorded_policy;
pub use codex_app_server_delivery_route::CodexAppServerDeliveryRoute;
pub use codex_app_server_scheduled_runs::CodexAppServerScheduledRuns;
pub use interaction_broker::{
    ApprovalDecisionError, InteractionHistoryError, InteractionHistoryRecord,
    InteractionHistoryState, QuestionHistoryState, QuestionResponse, RefusedApprovalOption,
    RefusedTypedApproval, ServiceInteractionBroker, TypedApprovalLegacyContext,
    TypedApprovalResolution, TypedApprovalSelection, TypedInteractionDecision,
    TypedInteractionDecisionOutcome,
};
mod acp_channel_listener;
pub use acp_channel_listener::AcpChannelListener;
mod unmaterialized_thread_holder;
pub use unmaterialized_thread_holder::UnmaterializedThreadHolder;
mod interaction_storage_failure;
pub mod stored_inventory_observation;
mod wakeup_timing_worker;
pub(crate) use interaction_storage_failure::map_interaction_storage_error;
mod push_handoff_worker;
pub use push_handoff_worker::{PushHandoffError, PushHandoffWorker};
pub mod received_mail_push;
pub use received_mail_push::received_mail_push;
mod outgoing_mail_message;
pub use outgoing_mail_message::outgoing_mail_message;
pub mod mail_push_preparation;
pub use mail_push_preparation::{MailPushPreparation, prepare_mail_push};
mod mail_delivery_session;
pub use mail_delivery_session::MailDeliverySession;
mod mail_delivery_projection;
pub use mail_delivery_projection::{
    project_cancelled_after_delivery, project_mail_cancel_outcome, project_mail_outcome,
    project_received_mail,
};
pub use wakeup_timing_worker::WakeTimingWorker;
mod native_thread_preparation;
mod schedule_timing_worker;
mod scheduled_native_dispatch;
mod scheduled_native_observation;
mod scheduled_run_worker;
mod wakeup_delivery_sender;
pub use schedule_timing_worker::ScheduleTimingWorker;
mod automation_configuration;
mod summary_native_worker;
pub use automation_configuration::{
    AutomationConfigurationBackend, AutomationConfigurationHandle, ConfigurationAdmissionLease,
};
pub mod delivery_reconciliation;
pub mod run_reconciliation;

mod project_replication_cursor;
pub use project_replication_cursor::project_replication_cursor;
pub mod observation_timestamp;

#[cfg(test)]
#[path = "../tests/support/native_worker_faults.rs"]
mod native_worker_faults;

mod central_automation_receiver;
mod remote_create_admission;
pub use remote_create_admission::{
    PreparedRemoteCreate, PreparedRemoteCreateActors, RemoteCreateAdmission,
    RemoteCreateAdmissionError, RemoteCreateAdmissionInputs, RemoteCreateConfigurationError,
};
mod central_automation_transport;
mod session_automation_transport;
pub use session_automation_transport::SessionAutomationTransport;
mod central_report_worker;
/// Test-only schedule execution fixtures shared with application inspection tests.
#[cfg(any(test, feature = "test-support"))]
pub mod scheduled_route_fixture;
/// Test-only hooks for causal runtime steps observed through the application.
#[cfg(feature = "test-support")]
pub mod test_support;
pub use central_automation_receiver::CentralAutomationReceiver;
pub use central_automation_transport::{
    CentralAutomationClock, CentralAutomationError, CentralAutomationTransport,
    SystemAutomationClock,
};
pub use central_report_worker::CentralReportWorker;
#[cfg(debug_assertions)]
mod debug_automation_faults;
#[cfg(debug_assertions)]
pub use automation_storage::AutomationFaultPoint;
#[cfg(debug_assertions)]
pub use debug_automation_faults::{
    AutomationFaultSelector, DebugAutomationFaultController, HeldAutomationFault,
};
#[cfg(all(test, debug_assertions))]
mod central_receiver_tests;
#[cfg(test)]
mod central_report_worker_tests;
