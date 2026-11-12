//! Resource-backed collaboration execution, provider adapters, durable owners and workers.
mod interaction_retention_worker;
pub use interaction_retention_worker::InteractionRetentionWorker;
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
    PROVIDER_OPERATION_RETENTION_MONTHS, ProviderOperationAdmission,
    ProviderOperationAdmissionResult, ProviderOperationRecord, ProviderOperationStore,
    ProviderOperationStoreError, provider_operation_retention_cutoff_ms,
};
pub use provider_session_event_hub::{HubReceiveError, ProviderSessionEventHub, receive_hub_event};
pub use provider_session_record::ProviderSessionRecord;
mod app_server_event_forwarding;
mod app_server_item_metadata;
mod app_server_item_translation;
mod app_server_model_catalog;
mod app_server_token_usage;
mod native_session_notifications;
mod native_user_receipts;
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
#[path = "app_server_item_translation_test.rs"]
mod app_server_item_translation_tests;
mod native_skill_reference;
mod provider_acp_content_translation;
mod provider_acp_event_projection;
mod provider_acp_interaction;
mod provider_acp_session_route;
mod router_session_app_server;
pub use app_server_model_catalog::{
    InvalidProviderModelEntry, ProviderModelEntry, ProviderModelReasoning, ProviderReasoningChoice,
    SessionModelCatalog, SessionModelCatalogError, render_model_list,
};
pub use router_session_app_server::{
    AppServerConnectionError, RouterSessionAppServerContext, RouterSessionAppServerListener,
    serve_router_session_app_server_connection,
};
mod session_delivery_router;
pub use provider_acp_session_route::ProviderAcpSessionRoute;
pub use session_delivery_router::SessionDeliveryRouter;
mod service_identity_storage;
pub use service_identity_storage::{
    load_installation_peer, load_service_identity, new_service_uuid, read_installation_peer,
    read_service_identity,
};
mod manifest_publication;
pub use manifest_publication::ManifestPublication;
mod native_control_backend;
mod session_display_name_cache;
pub use native_control_backend::NativeControlBackend;
pub use session_display_name_cache::SessionDisplayNameCache;
mod codex_app_server_delivery_route;
mod codex_queue_reconciliation;
mod interaction_broker;
mod message_effect_state;
pub use message_effect_state::classify_native_rejection;
mod native_message_dispatch;
mod native_recorded_policy;
pub use codex_app_server_delivery_route::CodexAppServerDeliveryRoute;
pub use interaction_broker::{
    ApprovalDecisionError, InteractionHistoryError, InteractionHistoryRecord,
    InteractionHistoryState, QuestionHistoryState, QuestionResponse, RefusedApprovalOption,
    RefusedTypedApproval, ServiceInteractionBroker, TypedApprovalLegacyContext,
    TypedApprovalResolution, TypedApprovalSelection, TypedInteractionDecision,
    TypedInteractionDecisionOutcome,
};
mod acp_channel_listener;
pub use acp_channel_listener::AcpChannelListener;
mod open_scratch_scopes;
pub use open_scratch_scopes::OpenScratchScopes;
mod unmaterialized_thread_holder;
pub use unmaterialized_thread_holder::UnmaterializedThreadHolder;
#[cfg(test)]
mod captured_warnings_test_support;
pub mod stored_inventory_observation;
/// The failed-pass retry lives with SQLite store maintenance; the layers above this crate
/// reach it here.
pub use sqlite_storage_maintenance::{FAILED_PASS_BACKOFF, FailedPassBackoff, FailureRetry};
mod wake_driven_worker_loop;
mod wake_driven_worker_observation;
pub use wake_driven_worker_loop::{BatchOutcome, WakeDrivenWorkerLoop, WakeSource, WorkerPass};
pub use wake_driven_worker_observation::WorkerObservation;
#[cfg(any(test, feature = "test-support"))]
pub use wake_driven_worker_observation::{LoopSnapshot, SleepGate};

pub mod observation_timestamp;

/// Test-only native boundary inputs used by the application's permission tests.
#[cfg(feature = "test-support")]
pub mod test_support;
