//! One optional descriptor read on the existing initialized connection.

use crate::{ModelMetadataSource, ProviderBehavior};
use agent_client_protocol::{Agent, ConnectionTo, UntypedMessage};
use provider_session_machines::{ModelReasoningMetadata, ModelReasoningUnknown};
use router_clock::{ClockInstant, RuntimeClock};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub(super) async fn read_connection_models(
    connection: &ConnectionTo<Agent>,
    behavior: &dyn ProviderBehavior,
    clock: &dyn RuntimeClock,
    deadline: ClockInstant,
    shutdown: &CancellationToken,
) -> ModelReasoningMetadata {
    let ModelMetadataSource::ConnectionRequest { method } = behavior.model_metadata_source() else {
        return ModelReasoningMetadata::Unknown(ModelReasoningUnknown::UnsupportedSource);
    };
    let remaining = deadline.saturating_duration_since(clock.monotonic_now());
    if remaining.is_zero() {
        return ModelReasoningMetadata::Unknown(ModelReasoningUnknown::Unavailable);
    }
    let Ok(request) = UntypedMessage::new(method, serde_json::json!({})) else {
        return ModelReasoningMetadata::Unknown(ModelReasoningUnknown::Malformed);
    };
    tokio::select! {
        biased;
        () = shutdown.cancelled() => ModelReasoningMetadata::Unknown(ModelReasoningUnknown::Unavailable),
        result = router_clock::timeout(clock, remaining.min(Duration::from_secs(2)), connection.send_request(request).block_task()) => match result {
            Ok(Ok(value)) => behavior.decode_model_metadata(&value),
            _ => ModelReasoningMetadata::Unknown(ModelReasoningUnknown::Unavailable),
        },
    }
}
