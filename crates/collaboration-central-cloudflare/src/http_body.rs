//! Complete each incoming HTTP upload before an edge route can refuse it.

use axum::body::Body;
use futures_util::TryStreamExt;

/// Match Axum's existing implicit JSON limit for every Worker HTTP route.
const MAX_HTTP_BODY_BYTES: usize = 2_097_152;

pub(crate) enum BodyCompletion {
    Complete(Body),
    OverLimit,
}

pub(crate) async fn complete(mut incoming: worker::Body) -> worker::Result<BodyCompletion> {
    let mut bytes = Vec::new();
    while let Some(chunk) = incoming.try_next().await? {
        if chunk.len() > MAX_HTTP_BODY_BYTES.saturating_sub(bytes.len()) {
            return Ok(BodyCompletion::OverLimit);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(BodyCompletion::Complete(Body::from(bytes)))
}

pub(crate) fn over_limit() -> worker::Result<worker::Response> {
    // The upload was not completed. The HTTP peer must not reuse its socket.
    let headers = worker::Headers::new();
    headers.set("Connection", "close")?;
    Ok(worker::Response::error("HTTP body exceeds 2097152 bytes", 413)?.with_headers(headers))
}
