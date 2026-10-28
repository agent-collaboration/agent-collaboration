//! Mail submit starts at the normal admitted session and registry owner.
#![cfg(feature = "conformance")]

#[path = "native_http/support.rs"]
mod configuration;

#[path = "mail_submit_http/grants.rs"]
mod grants;
#[path = "mail_submit_http/historical_piece.rs"]
mod historical_piece;
#[path = "support/mailbox_deadline_scenarios.rs"]
mod mailbox_deadline_scenarios;
#[path = "mail_submit_http/offline_deadlines.rs"]
mod offline_deadlines;

#[tokio::test]
async fn shared_offline_mailbox_alarm_oracle() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = grants::start(root.path(), &jwks).await?;
    let result = mailbox_deadline_scenarios::run(server.base_url()).await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    result
}
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "mail_submit_http/registry_setup.rs"]
mod registry_setup;
#[path = "mail_submit_http/relay.rs"]
mod relay;
#[path = "support/mail_relay_transport.rs"]
mod relay_transport;
#[path = "mail_submit_http/reopen.rs"]
mod reopen;
#[path = "support/mail_submit_scenarios.rs"]
mod scenarios;
#[path = "support/sender_control_scenarios.rs"]
mod sender_control_scenarios;
#[path = "mail_submit_http/sender_controls.rs"]
mod sender_controls;
#[path = "mail_submit_http/status_ack.rs"]
mod status_ack;

use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer, fixture_policy};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use std::time::Duration;

#[tokio::test]
async fn never_granted_project_epoch_refuses_items_and_cancels() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let proof = scenarios::run_ungranted_mail_refusal(server.base_url()).await;
    let stopped = server.shutdown().await;
    let jwks_stopped = jwks.shutdown().await;
    stopped?;
    jwks_stopped?;
    proof
}
