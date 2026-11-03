//! Shared mailbox rule oracles run unchanged on native and workerd stores.
#![cfg(feature = "conformance")]
#![deny(clippy::too_many_lines)]

#[path = "native_http/support.rs"]
mod configuration;
#[path = "support/mailbox_cancel_scenarios.rs"]
mod mailbox_cancel_scenarios;
#[path = "support/mailbox_capacity_scenarios.rs"]
mod mailbox_capacity_scenarios;
#[path = "support/mailbox_held_retention_scenario.rs"]
mod mailbox_held_retention_scenario;
#[path = "support/mailbox_identity_scenarios.rs"]
mod mailbox_identity_scenarios;
#[path = "support/mailbox_report_terminal_scenario.rs"]
mod mailbox_report_terminal_scenario;
#[path = "support/mailbox_rule_fixture.rs"]
mod mailbox_rule_fixture;
#[path = "support/mail_relay_transport.rs"]
mod relay_transport;

use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer, fixture_policy};
use collaboration_central_tokio::{DeliveryConfig, TokioServer, TokioSettings};

#[tokio::test]
async fn replay_and_conflict_keep_one_original_mailbox_item() -> ConformanceResult<()> {
    run_case(RuleCase::Identity).await
}
#[tokio::test]
async fn full_mailbox_refuses_before_custody_and_reserves_settlement() -> ConformanceResult<()> {
    run_case(RuleCase::Capacity).await
}
#[tokio::test]
async fn lost_custody_answer_keeps_cancel_pending_until_recipient_decides() -> ConformanceResult<()>
{
    run_case(RuleCase::Cancel).await
}
#[tokio::test]
async fn held_mailbox_payload_survives_history_horizon_and_releases_after_receipt()
-> ConformanceResult<()> {
    run_case(RuleCase::HeldRetention).await
}
#[tokio::test]
async fn invalid_preserved_prevention_report_stops_without_repeated_central_calls()
-> ConformanceResult<()> {
    run_case(RuleCase::ReportTerminal).await
}
enum RuleCase {
    ReportTerminal,
    HeldRetention,
    Identity,
    Capacity,
    Cancel,
}
async fn run_case(case: RuleCase) -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start_conformance(TokioSettings {
        root: root.path().into(),
        delivery: DeliveryConfig {
            relay_peer: "019f0000-0000-7000-8000-000000005240"
                .to_owned()
                .try_into()?,
        },
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    })
    .await?;
    let proof = match case {
        RuleCase::ReportTerminal => mailbox_report_terminal_scenario::run(server.base_url()).await,
        RuleCase::HeldRetention => mailbox_held_retention_scenario::run(server.base_url()).await,
        RuleCase::Identity => mailbox_identity_scenarios::run(server.base_url()).await,
        RuleCase::Capacity => mailbox_capacity_scenarios::run(server.base_url()).await,
        RuleCase::Cancel => mailbox_cancel_scenarios::run(server.base_url()).await,
    };
    let stopped = server.shutdown().await;
    let keys = jwks.shutdown().await;
    stopped?;
    keys?;
    proof
}
