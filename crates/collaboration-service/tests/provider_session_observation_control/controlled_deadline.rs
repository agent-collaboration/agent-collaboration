//! Drive the existing observation deadline after its real retained-event stream.
use collaboration_client::CollaborationClient;
use collaboration_protocol::{
    BoundedObservationRequest, BoundedObservationResult, ObservationEndReason,
};
use std::time::Duration;

pub(super) async fn observe_after_last_retained_event(
    client: &CollaborationClient,
    request: BoundedObservationRequest,
    last_sequence: u64,
) -> Result<BoundedObservationResult, Box<dyn std::error::Error>> {
    let deadline = Duration::from_secs(request.timeout_seconds);
    let (observed, mut streamed) = tokio::sync::mpsc::unbounded_channel();
    let observation = client.observe_provider_session_streaming(request, Some(observed));
    tokio::pin!(observation);
    loop {
        tokio::select! {
            result = &mut observation => {
                result?;
                return Err("observation ended before its retained events and controlled deadline".into());
            }
            event = streamed.recv() => {
                let event = event.ok_or("observation stream closed before its retained events")?;
                if event.event.get("sequence").and_then(serde_json::Value::as_u64) == Some(last_sequence) {
                    break;
                }
            }
        }
    }
    let pending_before_expiry = futures_util::poll!(&mut observation).is_pending();
    // The real request is attached and its deadline is armed. No IO is awaited
    // while paused; include Tokio's one-millisecond timer granularity.
    tokio::time::pause();
    tokio::time::advance(deadline + Duration::from_millis(1)).await;
    tokio::time::resume();
    let result = observation.await?;
    if !pending_before_expiry {
        return Err("observation ended before controlled expiry".into());
    }
    if result.end_reason != ObservationEndReason::DeadlineReached {
        return Err(format!(
            "observation did not reach its deadline: {:?}",
            result.end_reason
        )
        .into());
    }
    Ok(result)
}
