//! Central remote-create custody enters through real workerd machine sessions.
#![forbid(unsafe_code)]

#[cfg(debug_assertions)]
use collaboration_central_conformance::{
    AutomationClockControl, ConformanceClient, ConformanceResult, JwksFixtureServer,
    prepare_remote_create_restart, resume_remote_create_restart,
    run_remote_session_create_conformance,
};
#[cfg(debug_assertions)]
use collaboration_central_core::RegistryTime;
#[cfg(debug_assertions)]
#[path = "../../collaboration-central-conformance/examples/support/workerd_process.rs"]
#[expect(
    dead_code,
    reason = "This example uses configured startup; the shared harness also serves normal and invalid-relay startup in other examples."
)]
mod workerd_harness;
#[cfg(debug_assertions)]
use workerd_harness::{WorkerdProcess, WorkerdSettings};

#[cfg(debug_assertions)]
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
    let bundle = std::path::PathBuf::from(arguments.next().ok_or("debug alarm adapter bundle")?);
    if arguments.next().is_some() {
        return Err("unexpected workerd remote-create argument".into());
    }
    drop(std::net::TcpListener::bind(("127.0.0.1", port))?);
    let keys = JwksFixtureServer::start().await?;
    let settings = WorkerdSettings {
        node,
        miniflare,
        persistence,
        port,
        jwks_url: keys.url.clone(),
        relay_peer: "019f0000-0000-7000-8000-000000005240"
            .to_owned()
            .try_into()?,
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    };
    let server = match WorkerdProcess::start_configured(
        &settings,
        &settings.relay_peer.to_string(),
        Some(&bundle),
    )
    .await
    {
        Ok(server) => server,
        Err(error) => {
            keys.shutdown().await?;
            return Err(error);
        }
    };
    let base = format!("http://127.0.0.1:{port}");
    let proof = async {
        let client = ConformanceClient::new(&base)?;
        client
            .automation_clock(AutomationClockControl::Start {
                now: RegistryTime("2026-10-31T10:00:01.123Z".parse()?),
            })
            .await?;
        run_remote_session_create_conformance(&client).await?;
        prepare_remote_create_restart(&client).await
    }
    .await;
    let stopped = server.stop().await;
    let resumed: ConformanceResult<()> = async {
        let prepared = proof?;
        stopped?;
        let server = WorkerdProcess::start_configured(
            &settings,
            &settings.relay_peer.to_string(),
            Some(&bundle),
        )
        .await?;
        let proof = resume_remote_create_restart(&ConformanceClient::new(&base)?, prepared).await;
        let stopped = server.stop().await;
        proof?;
        stopped?;
        Ok(())
    }
    .await;
    let keys = keys.shutdown().await;
    resumed?;
    keys?;
    println!(
        "REMOTE_SESSION_CREATE_RESULT shared central custody and retention-restart passed; native effects unverified"
    );
    Ok(())
}

#[cfg(not(debug_assertions))]
fn main() {
    eprintln!("remote-create central verification uses the debug-only controlled clock");
}
