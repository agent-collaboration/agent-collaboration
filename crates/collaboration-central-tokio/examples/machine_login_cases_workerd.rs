//! Every B3 machine-login SQL case on real Durable Object SQLite: the same
//! cases core runs on the Tokio store (`SQL_CASES`), each on its own named
//! object. Each case checks the store's own rows itself and answers only
//! whether it passed. `persistence` writes in one request and reads back in a
//! later request to the same named object. A workerd process restart is not
//! part of this run.
#![forbid(unsafe_code)]

use collaboration_central_conformance::workerd_process::{WorkerdProcess, WorkerdSettings};
use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer, owner_assertion};
use collaboration_central_core::organization_authority::{CasePhase, SQL_CASES};
use durable_steps::StoreError;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reply {
    result: Result<(), StoreError>,
    reply_loss_observed: bool,
}

async fn run_case(
    http: &reqwest::Client,
    url: &str,
    case: &str,
    phase: CasePhase,
) -> ConformanceResult<()> {
    let reply: Reply = http
        .post(url)
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&json!({
            "fixture": "machineLogin",
            "command": {"kind": "machineLogin", "value": {"case": case, "phase": phase, "object": case}},
            "fault": null,
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if reply.reply_loss_observed {
        return Err(format!("{case} observed a lost reply it never asked for").into());
    }
    Ok(reply.result?)
}

async fn run_all(http: &reqwest::Client, url: &str) -> ConformanceResult<usize> {
    for case in SQL_CASES {
        if *case == "persistence" {
            run_case(http, url, case, CasePhase::Write).await?;
            run_case(http, url, case, CasePhase::Read).await?;
        } else {
            run_case(http, url, case, CasePhase::Run).await?;
        }
        println!("MACHINE_LOGIN_CASE {case} passed");
    }
    Ok(SQL_CASES.len())
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
    let result = run_all(&http, &url).await;
    let stopped = server.stop().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    let passed = result?;
    println!(
        "MACHINE_LOGIN_RESULT {passed} cases passed (real DO SQLite; the staged, unregistered B3 corpus; persistence within one workerd run, no process restart)"
    );
    Ok(())
}
