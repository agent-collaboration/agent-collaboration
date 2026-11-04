use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, enroll_admitted_peer, fixture_policy,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_sync_lease::{LeasePolicy, RegistryConfig};
use sqlx_core::{query::query, row::Row};
use std::{collections::BTreeSet, time::Duration};

#[expect(
    clippy::panic_in_result_fn,
    reason = "Schema assertions fail the test; storage errors propagate."
)]
#[tokio::test]
async fn normal_fleet_and_copy_bootstrap_omit_retired_delivery_tables_after_reopen()
-> ConformanceResult<()> {
    // Literal role contract at SDK 2be7372: seven obsolete relay tables.
    const RETIRED: [&str; 7] = [
        "_fleet_sync_mail_cancel_links",
        "_fleet_sync_mail_controls",
        "_fleet_sync_mail_items",
        "_fleet_sync_mail_state",
        "_fleet_sync_delivery_receipts",
        "_fleet_sync_inbox_state",
        "_fleet_sync_outbox_status",
    ];
    let directory = tempfile::tempdir()?;
    for _ in 0..2 {
        // The second iteration reopens these exact owning stores and runs the
        // normal Fleet and Copy initializers again, with existing history.
        let fixture = super::composed_fleet_fixture::Fixture::open(
            directory.path(),
            super::composed_fleet_fixture::NOW,
        )
        .await?;
        let proof = async {
            for (store, required, forbidden) in [
                (
                    &fixture.store,
                    "_fleet_sync_outbox_items",
                    "_fleet_sync_copy",
                ),
                (
                    &fixture.project_store,
                    "_fleet_sync_copy",
                    "_fleet_sync_outbox_items",
                ),
            ] {
                let tables: BTreeSet<String> = store
                    .transaction(|scope| {
                        Box::pin(async move {
                            query::<sqlx_turso::Turso>(
                                "SELECT name FROM sqlite_master WHERE type='table'",
                            )
                            .fetch_all(scope.executor())
                            .await
                            .map_err(StoreError::backend)?
                            .into_iter()
                            .map(|row| row.try_get("name").map_err(StoreError::backend))
                            .collect()
                        })
                    })
                    .await?;
                assert!(
                    tables.contains(required),
                    "normal role bootstrap omitted {required}"
                );
                assert!(
                    !tables.contains(forbidden),
                    "role installed another owner's corpus: {forbidden}"
                );
                for name in RETIRED {
                    assert!(
                        !tables.contains(name),
                        "retired table survived bootstrap: {name}"
                    );
                }
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
        let closed = fixture.close().await;
        closed?;
        proof?;
    }
    Ok(())
}

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

        // R1's complete physical-store inventory has no per-machine exception.
        let stores = std::fs::read_dir(directory.path())?
            .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
            .collect::<Result<BTreeSet<_>, std::io::Error>>()?;
        let central_stores: BTreeSet<_> = stores
            .into_iter()
            .filter(|name| name.ends_with(".db"))
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
