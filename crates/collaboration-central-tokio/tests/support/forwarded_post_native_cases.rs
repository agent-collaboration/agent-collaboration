//! Own native server lifetimes around the same normal-session Worker scenarios.
#![deny(clippy::too_many_lines)]
use super::forwarded_post_scenarios::{self, ForwardCase};
use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer, fixture_policy};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use std::time::Duration;

pub(super) async fn run(case: ForwardCase) -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let mut server = None;
    let proof = async {
        server = Some(
            TokioServer::start_conformance(TokioSettings {
                root: root.path().into(),
                access_policy: fixture_policy(),
                jwks_url: jwks.url.clone(),
                registry: super::configuration::registry_config()?,
                delivery: collaboration_central_tokio::DeliveryConfig {
                    relay_peer: "019f0000-0000-7000-8000-000000005240"
                        .to_owned()
                        .try_into()?,
                },
                holder_answer_timeout: Duration::from_secs(2),
                copy_ack_wait: Duration::from_secs(2),
            })
            .await?,
        );
        forwarded_post_scenarios::run(
            server.as_ref().ok_or("native server absent")?.base_url(),
            case,
            |base, _service, credential, _now| {
                super::forwarded_post_wire::connect(base, credential)
            },
        )
        .await
    }
    .await;
    let stopped = match server.take() {
        Some(server) => server.shutdown().await.map_err(Into::into),
        None => Ok(()),
    };
    let keys = jwks.shutdown().await;
    let failures = [
        ("scenario", proof),
        ("server cleanup", stopped),
        ("JWKS cleanup", keys),
    ]
    .into_iter()
    .filter_map(|(phase, result)| result.err().map(|error| format!("{phase}: {error}")))
    .collect::<Vec<_>>();
    if failures.is_empty() {
        Ok(())
    } else {
        eprintln!(
            "forwarding boundary evidence retained: {}",
            root.keep().display()
        );
        Err(failures.join("; ").into())
    }
}
