//! The normal consumer session reads paged receipts from the real Fleet DO.
#![forbid(unsafe_code)]

use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer};
#[path = "../tests/support/fleet_effects_scenarios.rs"]
mod scenarios;

#[path = "../../collaboration-central-conformance/examples/support/workerd_process.rs"]
mod workerd_harness;
use workerd_harness::{WorkerdProcess, WorkerdSettings};

#[tokio::main]
async fn main() -> ConformanceResult<()> {
    let mut arguments = std::env::args().skip(1);
    let node = arguments.next().ok_or("Node executable")?.into();
    let miniflare = arguments
        .next()
        .ok_or("pinned Miniflare entrypoint")?
        .into();
    let persistence = arguments
        .next()
        .ok_or("fresh persistence directory")?
        .into();
    let port = arguments.next().ok_or("loopback port")?.parse::<u16>()?;
    drop(std::net::TcpListener::bind(("127.0.0.1", port))?);
    let jwks = JwksFixtureServer::start().await?;
    let settings = WorkerdSettings {
        relay_peer: fleet_sync_protocol::PeerId::try_from(
            "019f0000-0000-7000-8000-000000005240".to_owned(),
        )?,
        node,
        miniflare,
        persistence,
        port,
        jwks_url: jwks.url.clone(),
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    };
    let base = format!("http://127.0.0.1:{port}");
    let server = match WorkerdProcess::start(&settings).await {
        Ok(server) => server,
        Err(error) => {
            jwks.shutdown().await?;
            return Err(error);
        }
    };
    let result = async {
        scenarios::seed(&base).await?;
        scenarios::pages_and_refusals(&base, false).await?;
        println!("FLEET_EFFECTS_CASE normal_session_reads_300_effects_and_typed_unknown_scope_cursor_refusals passed");
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let stopped = server.stop().await;
    if let Err(error) = result {
        jwks.shutdown().await?;
        return Err(error);
    }
    if let Err(error) = stopped {
        jwks.shutdown().await?;
        return Err(error);
    }
    let server = match WorkerdProcess::start(&settings).await {
        Ok(server) => server,
        Err(error) => {
            jwks.shutdown().await?;
            return Err(error);
        }
    };
    let result = scenarios::pages_and_refusals(&base, true).await;
    let stopped = server.stop().await;
    let fixture = jwks.shutdown().await;
    result?;
    stopped?;
    fixture?;
    println!(
        "FLEET_EFFECTS_CASE whole_worker_reopen_retains_receipts_and_raw_ordinal_continuation passed"
    );
    println!(
        "FLEET_EFFECTS_RESULT 2 distinct groups passed; 0 failed; 0 ignored (normal session read; fixture insertion is owner-only conformance)"
    );
    Ok(())
}
