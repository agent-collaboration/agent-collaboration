//! Actual attachment faults must not poison other sockets in the Fleet object.
use super::{normal_capability_cases::observe, normal_session_support::NormalSession};
use collaboration_central_conformance::ConformanceResult;
use serde_json::json;

pub(super) async fn run(base: &str) -> ConformanceResult<()> {
    let mut healthy = NormalSession::attached(base, "healthy registration survivor").await?;
    for fault in ["missing", "invalid"] {
        let dead = NormalSession::attached(base, &format!("isolated {fault} attachment")).await?;
        let snapshot = observe(
            base,
            json!({"kind":"sessionAttachmentFault","value":{
                "service":dead.service,"connection":dead.connection,"fault":fault
            }}),
        )
        .await?;
        let registrations = snapshot
            .as_object()
            .ok_or("registration snapshot is not an object")?;
        if registrations.contains_key(&dead.connection)
            || registrations.get(&healthy.connection)
                != Some(
                    &json!({"kind":"attached","sessionId":healthy.session,"schemaSupport":{"maxVersion":1}}),
                )
        {
            return Err(format!(
                "{fault} attachment poisoned the healthy registration: {snapshot}"
            )
            .into());
        }
        healthy.hello().await?;
        healthy.welcome().await?;
        let replacement = dead
            .reconnect(base, &format!("replacement after {fault} attachment"))
            .await?;
        replacement.close().await?;
        println!(
            "SESSION_ISOLATION_CASE {fault}_attachment_keeps_healthy_socket_and_replacement passed"
        );
    }
    healthy.close().await?;
    Ok(())
}
