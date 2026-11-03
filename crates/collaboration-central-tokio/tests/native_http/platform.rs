use std::sync::Arc;
#[path = "support.rs"]
mod configuration;
use configuration::{TestError, registry_config};

pub async fn native_platform(
    key_source: &str,
) -> Result<
    (
        collaboration_central_tokio::TokioCentralPlatform,
        tempfile::TempDir,
    ),
    TestError,
> {
    let directory = tempfile::tempdir()?;
    let platform = collaboration_central_tokio::TokioCentralPlatform::new(
        key_source,
        Arc::new(collaboration_central_core::ConformanceAccessClock::new()),
        |_| {},
        directory.path(),
        registry_config()?,
        collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        std::time::Duration::from_secs(2),
        std::time::Duration::from_secs(2),
    )
    .await?;
    Ok((platform, directory))
}
