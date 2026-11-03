//! Complete each incoming HTTP upload before an edge route can refuse it.

use axum::body::Body;
use futures_util::TryStreamExt;

/// Match Axum's existing implicit JSON limit for every Worker HTTP route.
const MAX_HTTP_BODY_BYTES: usize = 2_097_152;

pub(crate) enum BodyCompletion {
    Complete(Body),
    OverLimit,
}

#[cfg(feature = "conformance")]
static BODY_ORDER: std::sync::LazyLock<collaboration_central_core::BodyOrderProbe> =
    std::sync::LazyLock::new(collaboration_central_core::BodyOrderProbe::default);

#[cfg(feature = "conformance")]
pub(crate) fn capture(
    request: &worker::Request,
) -> Option<collaboration_central_core::BodyOrderCapture> {
    let id: collaboration_central_core::BodyObservationId = request
        .headers()
        .get(collaboration_central_core::BODY_ORDER_HEADER)
        .ok()??
        .try_into()
        .ok()?;
    BODY_ORDER.capture(&id)
}

#[cfg(feature = "conformance")]
pub(crate) async fn control(
    command: collaboration_central_core::BodyOrderCommand,
) -> worker::Result<worker::Response> {
    worker::Response::from_json(&BODY_ORDER.control(command).await)
}

pub(crate) async fn complete(
    mut incoming: worker::Body,
    #[cfg(feature = "conformance")] capture: Option<&collaboration_central_core::BodyOrderCapture>,
) -> worker::Result<BodyCompletion> {
    let mut bytes = Vec::new();
    while let Some(chunk) = incoming.try_next().await? {
        #[cfg(feature = "conformance")]
        if let Some(capture) = capture {
            capture.record_read(chunk.len());
        }
        if chunk.len() > MAX_HTTP_BODY_BYTES.saturating_sub(bytes.len()) {
            return Ok(BodyCompletion::OverLimit);
        }
        bytes.extend_from_slice(&chunk);
    }
    #[cfg(feature = "conformance")]
    if let Some(capture) = capture {
        capture.record_end();
    }
    Ok(BodyCompletion::Complete(Body::from(bytes)))
}

pub(crate) fn over_limit() -> worker::Result<worker::Response> {
    // The upload was not completed. The HTTP peer must not reuse its socket.
    let headers = worker::Headers::new();
    headers.set("Connection", "close")?;
    Ok(worker::Response::error("HTTP body exceeds 2097152 bytes", 413)?.with_headers(headers))
}
