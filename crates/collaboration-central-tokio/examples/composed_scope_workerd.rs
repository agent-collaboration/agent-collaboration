//! Modern shared scenarios use one real Worker process wrapper and physical reopen.
#![forbid(unsafe_code)]

#[path = "../../collaboration-central-conformance/examples/support/workerd_process.rs"]
mod workerd_process;
use collaboration_central_conformance::{ConformanceClient, ConformanceResult, JwksFixtureServer};
use workerd_process::{WorkerdProcess, WorkerdSettings};

#[tokio::main]
async fn main() -> ConformanceResult<()> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().ok_or("composed-copy-suite, composed-transactions, composed-numeric, runtime-controls or composed-fleet-conflict")?;
    if !matches!(
        mode.as_str(),
        "composed-copy-suite"
            | "composed-transactions"
            | "composed-numeric"
            | "runtime-controls"
            | "composed-fleet-conflict"
    ) {
        return Err("unknown composed scenario".into());
    }
    let node = args.next().ok_or("Node executable")?.into();
    let miniflare = args.next().ok_or("pinned Miniflare entrypoint")?.into();
    let persistence = args.next().ok_or("isolated persistence directory")?.into();
    let port = args.next().ok_or("loopback port")?.parse()?;
    if args.next().is_some() {
        return Err("unexpected composed arguments".into());
    }
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
    let client = ConformanceClient::new(&format!("http://127.0.0.1:{}", settings.port))?;
    let server = match WorkerdProcess::start(&settings).await {
        Ok(server) => server,
        Err(error) => {
            jwks.shutdown().await?;
            return Err(error);
        }
    };
    if mode == "composed-copy-suite" {
        let server = std::sync::Arc::new(tokio::sync::Mutex::new(Some(server)));
        let settings = std::sync::Arc::new(settings);
        let proof = collaboration_central_conformance::run_composed_copy_suite(client.clone(), {
            let server = server.clone();
            let client = client.clone();
            move || {
                let server = server.clone();
                let settings = settings.clone();
                let client = client.clone();
                async move {
                    let mut slot = server.lock().await;
                    slot.take()
                        .ok_or("workerd restart without process")?
                        .stop()
                        .await?;
                    *slot = Some(WorkerdProcess::start(&settings).await?);
                    Ok(client)
                }
            }
        })
        .await;
        if let Some(server) = server.lock().await.take() {
            server.stop().await?;
        }
        jwks.shutdown().await?;
        return proof;
    }
    if mode == "composed-transactions" {
        let prepared =
            collaboration_central_conformance::ComposedTransactionScenario::prepare(&client).await;
        server.stop().await?;
        let proof = async {
            let prepared = prepared?;
            let restarted = WorkerdProcess::start(&settings).await?;
            let proof = prepared.verify(&client).await;
            restarted.stop().await?;
            proof
        }
        .await;
        jwks.shutdown().await?;
        return proof;
    }
    if mode == "runtime-controls" {
        let proof =
            collaboration_central_conformance::run_runtime_control_conformance(&client).await;
        server.stop().await?;
        jwks.shutdown().await?;
        return proof;
    }
    if mode == "composed-fleet-conflict" {
        let proof = collaboration_central_conformance::run_composed_fleet_conflict(&client).await;
        server.stop().await?;
        jwks.shutdown().await?;
        return proof;
    }
    if mode == "composed-numeric" {
        let prepared =
            collaboration_central_conformance::ComposedNumericScenario::prepare(&client).await;
        server.stop().await?;
        let proof = async {
            let prepared = prepared?;
            let restarted = WorkerdProcess::start(&settings).await?;
            let proof = prepared.verify(&client).await;
            restarted.stop().await?;
            proof
        }
        .await;
        jwks.shutdown().await?;
        return proof;
    }
    server.stop().await?;
    jwks.shutdown().await?;
    Err("composed scenario was not dispatched".into())
}
