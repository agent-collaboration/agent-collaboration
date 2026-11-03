use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, enroll_admitted_peer, fixture_policy,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use fleet_sync_lease::{LeasePolicy, RegistryConfig};
use std::{collections::BTreeSet, time::Duration};

#[tokio::test]
async fn three_machine_sockets_share_the_fleet_store() -> ConformanceResult<()> {
    // Arrange: the HTTP enrollment and WebSocket admission paths remain real.
    let jwks = JwksFixtureServer::start().await?;
    let directory = tempfile::tempdir()?;
    let server = TokioServer::start(TokioSettings {
        root: directory.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: "019f0000-0000-7000-8000-000000005240"
                .to_owned()
                .try_into()?,
        },
        registry: RegistryConfig {
            authoritative_receiver: "019f0000-0000-7000-8000-000000000010"
                .to_owned()
                .try_into()?,
            lease: LeasePolicy {
                lease_duration: Duration::from_secs(14_400),
                renew_interval: Duration::from_secs(900),
            },
            release_unsent_for: Duration::ZERO,
            move_needs_renewal_within: Some(Duration::from_secs(1800)),
        },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let proof = async {
        let client = ConformanceClient::new(server.base_url())?;
        let mut peers = Vec::new();

        // Act: every returned socket has received its real Welcome.
        for (service, peer, credential, session) in [
            (
                "019f0000-0000-7000-8000-000000008101",
                "019f0000-0000-7000-8000-000000008111",
                "d1-layout-a",
                "019f0000-0000-7000-8000-000000008121",
            ),
            (
                "019f0000-0000-7000-8000-000000008102",
                "019f0000-0000-7000-8000-000000008112",
                "d1-layout-b",
                "019f0000-0000-7000-8000-000000008122",
            ),
            (
                "019f0000-0000-7000-8000-000000008103",
                "019f0000-0000-7000-8000-000000008113",
                "d1-layout-c",
                "019f0000-0000-7000-8000-000000008123",
            ),
        ] {
            peers.push(enroll_admitted_peer(&client, service, peer, credential, session).await?);
        }

        // Assert: every live attachment belongs to the Fleet's real host.
        let inventory = server
            .conformance_platform()
            .conformance_socket_inventory()
            .await?;
        let mut fleet_admissions = Vec::new();
        for owner in inventory {
            if owner.address == collaboration_central_core::ObjectAddress::Fleet {
                fleet_admissions.extend(owner.admissions);
            } else if !owner.admissions.is_empty() {
                return Err(format!("machine sockets belong to {:?}", owner.address).into());
            }
        }
        let admitted_services: BTreeSet<_> = fleet_admissions
            .iter()
            .map(|origin| origin.installation.as_str().to_owned())
            .collect();
        let expected_services = BTreeSet::from([
            "019f0000-0000-7000-8000-000000008101".to_owned(),
            "019f0000-0000-7000-8000-000000008102".to_owned(),
            "019f0000-0000-7000-8000-000000008103".to_owned(),
        ]);
        if fleet_admissions.len() != 3 || admitted_services != expected_services {
            return Err(format!("Fleet lost a machine attachment: {admitted_services:?}").into());
        }

        // R1's physical stores, with C4's explicit intermediate Mailbox exception.
        let stores = std::fs::read_dir(directory.path())?
            .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
            .collect::<Result<BTreeSet<_>, std::io::Error>>()?;
        let central_stores: BTreeSet<_> = stores
            .into_iter()
            .filter(|name| name.ends_with(".db") && !name.starts_with("mailbox-"))
            .collect();
        if central_stores != BTreeSet::from(["fleet.db".to_owned()]) {
            return Err(format!(
                "machine admission created stores outside Fleet: {central_stores:?}"
            )
            .into());
        }
        for peer in peers {
            peer.socket
                .close(reqwest_websocket::CloseCode::Normal, None)
                .await?;
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    proof
}
