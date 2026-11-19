//! The production Tokio central link route on simulated TCP.
use collaboration_central_tokio::{TokioWorkspaceHost, WorkspaceLink};
use hyper::server::conn::http1;
use hyper_util::{rt::TokioIo, service::TowerToHyperService};
use std::{
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use turmoil::net::TcpListener;

/// Runs admission, the WebSocket handler and storage through the real Tokio
/// central. The caller owns host provisioning and must inject the simulated
/// clock when opening the host. This drains the link before returning so the
/// caller can close the stores.
pub async fn serve_tokio_central(
    host: Arc<TokioWorkspaceHost>,
    port: u16,
    stop: CancellationToken,
) -> turmoil::Result {
    let listener = TcpListener::bind((IpAddr::from(Ipv4Addr::UNSPECIFIED), port)).await?;
    let link = WorkspaceLink::new(host);
    let router = link.router();
    let mut connections = JoinSet::new();
    let result = loop {
        tokio::select! {
            biased;
            () = stop.cancelled() => break Ok(()),
            Some(finished) = connections.join_next(), if !connections.is_empty() => {
                if let Err(error) = finished {
                    break Err(error.into());
                }
            }
            accepted = listener.accept() => {
                let (stream, _) = match accepted {
                    Ok(accepted) => accepted,
                    Err(error) => break Err(error.into()),
                };
                let service = TowerToHyperService::new(router.clone());
                let stop = stop.clone();
                connections.spawn_local(async move {
                    let connection = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .with_upgrades();
                    tokio::select! {
                        () = stop.cancelled() => {}
                        // A simulated disconnect can interrupt the HTTP handshake.
                        _ = connection => {}
                    }
                });
            }
        }
    };
    link.begin_shutdown();
    connections.shutdown().await;
    link.shutdown().await;
    result
}
