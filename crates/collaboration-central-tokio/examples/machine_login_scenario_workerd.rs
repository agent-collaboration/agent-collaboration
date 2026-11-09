//! B3's machine-login scenario on real Durable Object SQLite. The Worker runs
//! core's shared script (`run_scenario`) on a fresh object: a Fleet built by
//! 0001/0002 code, the staged switch, then the machine-login decisions through
//! their public entries. Its report must equal `expected_scenario`, the same
//! report the Tokio store produces in core's own test.
#![forbid(unsafe_code)]

use collaboration_central_conformance::workerd_process::{WorkerdProcess, WorkerdSettings};
use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer, owner_assertion};
use collaboration_central_core::organization_authority::{ScenarioStep, expected_scenario};
use durable_steps::StoreError;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reply {
    result: Result<Vec<ScenarioStep>, StoreError>,
    reply_loss_observed: bool,
}

async fn scenario(http: &reqwest::Client, url: &str) -> ConformanceResult<Vec<ScenarioStep>> {
    let reply: Reply = http
        .post(url)
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&json!({"fixture":"machineLogin","command":{"kind":"machineLogin","value":{}},"fault":null}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if reply.reply_loss_observed {
        return Err("the scenario observed a lost reply it never asked for".into());
    }
    Ok(reply.result?)
}

fn compare(report: &[ScenarioStep]) -> ConformanceResult<()> {
    let expected = expected_scenario();
    for (observed, wanted) in report.iter().zip(&expected) {
        if observed != wanted {
            return Err(format!(
                "step {:?} differs from Tokio's expectation:\n observed {}\n expected {}",
                wanted.name, observed.observed, wanted.observed
            )
            .into());
        }
        println!("MACHINE_LOGIN_STEP {} matched", wanted.name);
    }
    if report.len() != expected.len() {
        return Err(format!(
            "the report has {} steps; the expectation has {}",
            report.len(),
            expected.len()
        )
        .into());
    }
    Ok(())
}

#[tokio::main]
async fn main() -> ConformanceResult<()> {
    let mut args = std::env::args().skip(1);
    let node = args.next().ok_or("Node executable")?.into();
    let miniflare = args.next().ok_or("pinned Miniflare entrypoint")?.into();
    let persistence = args.next().ok_or("fresh persistence")?.into();
    let port = args.next().ok_or("port")?.parse::<u16>()?;
    if args.next().is_some() {
        return Err("unexpected arguments".into());
    }
    drop(std::net::TcpListener::bind(("127.0.0.1", port))?);
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()?;
    let url = format!("http://127.0.0.1:{port}/admin/v1/conformance/bootstrap");
    let jwks = JwksFixtureServer::start().await?;
    let settings = WorkerdSettings {
        fixed_access_time: None,
        legacy_organization: collaboration_central_conformance::legacy_organization(),
        relay_peer: fleet_sync_protocol::PeerId::try_from(
            "019f0000-0000-7000-8000-000000005240".to_owned(),
        )?,
        node,
        miniflare,
        persistence,
        port,
        jwks_url: jwks.url.clone(),
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    };
    let server = WorkerdProcess::start(&settings).await?;
    let result = scenario(&http, &url).await;
    let stopped = server.stop().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    let report = result?;
    compare(&report)?;
    println!(
        "MACHINE_LOGIN_RESULT {} steps matched (real DO SQLite; the staged, unregistered B3 corpus; fixture initialization only)",
        report.len()
    );
    Ok(())
}
