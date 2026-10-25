//! Archive notices use Fleet's verified ServiceId projection, with one notice per holder.
use super::{session_fixture, support::*};
use board_sync_protocol::{BoardNotice, BoardReadPage, FleetFeedRead};
use collaboration_central_core::{AccessCaller, OwnerIdentity};
use fleet_admission::FleetOperation;
use fleet_sync_lease::{DatabaseLifecycle, RegistryGrant, RegistryRevision, RegistrySnapshot};
use fleet_sync_protocol::{DatabaseId, Epoch, ImportDigest, Origin, PeerId};
use message_board::{BoardId, FleetPosition};
use runtime_primitives::{Clock, SyncTime};
use serde_json::{Value, json};
use std::{num::NonZeroU64, sync::Arc};
const BOARD: &str = "019f0000-0000-7000-8000-000000004973";
const PROJECTS: [&str; 2] = [
    "019f0000-0000-7000-8000-000000004974",
    "019f0000-0000-7000-8000-000000004975",
];
async fn registry(fixture: &Fixture, kind: &str, request: Value) -> TestResult<Value> {
    let bytes=fixture.runtime.on_call(serde_json::to_vec(&json!({
        "source":{"kind":"registry"},"sourceFace":"registry","authority":{"kind":"object"},
        "destination":{"kind":"fleet"},"destinationFace":"fleet","identity":{"operation":durable_steps::Uuid::now_v7(),"step":"archive_projection"},
        "kind":kind,"target":null,"request":request
    }))?).await.map_err(|error|format!("Registry projection call failed: {error:?}"))?;
    Ok(serde_json::from_slice(&bytes)?)
}
fn projection(project: &str, incarnation: &Value, held: bool, now: SyncTime) -> TestResult<Value> {
    let epoch = Epoch::new(1)?;
    let snapshot = RegistrySnapshot {
        database: DatabaseId::try_from(project.to_owned())?,
        revision: RegistryRevision {
            epoch,
            change: NonZeroU64::new(if held { 1 } else { 2 }).ok_or("positive revision")?,
        },
        changed_at: now,
        lifecycle: if held {
            DatabaseLifecycle::Registered {
                origin: Origin::Empty,
                imported_from: None,
            }
        } else {
            DatabaseLifecycle::Unheld {
                import_digest: ImportDigest::new("a".repeat(64))?,
            }
        },
        grant: if held {
            Some(RegistryGrant {
                epoch,
                holder: PeerId::try_from("019f0000-0000-7000-8000-000000004976".to_owned())?,
                expires_at: SyncTime::from_unix_millis(now.unix_millis() + 3_600_000),
            })
        } else {
            None
        },
    };
    let holder = if held { Some(installation()?) } else { None };
    let digest = ring::digest::digest(
        &ring::digest::SHA256,
        &serde_json::to_vec(&(&snapshot, &holder))?,
    )
    .as_ref()
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect::<String>();
    Ok(json!({"snapshot":snapshot,"holder":holder,"incarnation":incarnation,"digest":digest}))
}
#[tokio::test]
async fn board_archiving_notifies_a_known_holder_once_and_final_unheld_publication_hints_the_feed()
-> TestResult {
    let root = tempfile::tempdir()?;
    let delivery_root = tempfile::tempdir()?;
    let delivery = Arc::new(
        session_fixture::Fixture::open_for_installation(delivery_root.path(), installation()?)
            .await?,
    );
    let io = Io::new(delivery.clone());
    let fixture = Fixture::open(root.path(), io.clone()).await?;
    let owner = OwnerIdentity::from_access(AccessCaller::Owner {
        email: "fixture@example.test".into(),
    })?;
    let records = vec![
        json!({"kind":"workspace","id":WORKSPACE,"name":"Archive notice source","description":"actual public import","repositories":[],"time":{"kind":"legacyUnknown"}}),
        json!({"kind":"board","id":BOARD,"workspace":WORKSPACE,"name":"Held board","description":"two projects one holder","state":"active","time":{"kind":"legacyUnknown"}}),
        json!({"kind":"directory","project":PROJECTS[0],"board":BOARD,"title":"First held project","time":{"kind":"legacyUnknown"}}),
        json!({"kind":"directory","project":PROJECTS[1],"board":BOARD,"title":"Second held project","time":{"kind":"legacyUnknown"}}),
    ];
    fixture.runtime.on_call(serde_json::to_vec(&json!({
        "source":{"kind":"registry"},"sourceFace":"edge","authority":{"kind":"owner","value":owner},
        "destination":{"kind":"fleet"},"destinationFace":"fleet","identity":{"operation":durable_steps::Uuid::now_v7(),"step":"import_archive_fixture"},
        "kind":"central.object","target":null,"request":{"kind":"fleet","value":{"kind":"import","value":{"bundle":{"provenance":{"installation":SERVICE,"machine":"archive owner"},"declaredOwner":"owner","records":records}}}}
    }))?).await.map_err(|error|format!("archive import failed: {error:?}"))?;
    let incarnation = registry(&fixture, "central.lease-incarnation", json!({"atLeast":0})).await?;
    let unheld_at = io.now();
    for project in PROJECTS {
        assert_eq!(
            registry(
                &fixture,
                "central.lease-projection",
                projection(project, &incarnation, true, io.now())?
            )
            .await?,
            json!({"kind":"applied"})
        );
    }
    let board = BoardId::try_from(BOARD.to_owned())?;
    let archive = offered(FleetOperation::BoardArchive {
        board: board.clone(),
    })?;
    let answer = fixture.decide(&archive).await?;
    assert_accepted(&answer)?;
    assert!(
        io.calls()?.is_empty(),
        "archive dispatched before its own alarm"
    );
    fixture.deliver().await?;
    assert_eq!(
        notice_kinds(&delivery).await?,
        vec![BoardNotice::BoardArchiving {
            board: board.clone()
        }]
    );
    assert_eq!(fixture.decide(&archive).await?, answer);
    assert!(fixture.runtime.on_alarm().await?.completed.is_empty());
    assert!(
        matches!(fixture.feed(6).await?,BoardReadPage::FleetFeed(FleetFeedRead::Page {records,head,..}) if records.is_empty() && head.get()==6)
    );
    for project in PROJECTS {
        registry(
            &fixture,
            "central.lease-projection",
            projection(project, &incarnation, false, unheld_at)?,
        )
        .await?;
    }
    fixture.deliver().await?;
    assert_eq!(
        notice_kinds(&delivery).await?,
        vec![
            BoardNotice::BoardArchiving { board },
            BoardNotice::FleetFeedAdvanced {
                through: FleetPosition::try_from(7)?
            }
        ]
    );
    assert!(
        matches!(fixture.feed(6).await?,BoardReadPage::FleetFeed(FleetFeedRead::Page {records,head,..}) if records.len()==1 && head.get()==7)
    );
    assert_eq!(
        registry(
            &fixture,
            "central.lease-projection",
            projection(PROJECTS[1], &incarnation, false, unheld_at)?
        )
        .await?,
        json!({"kind":"duplicate"})
    );
    assert!(fixture.runtime.on_alarm().await?.completed.is_empty());
    assert_eq!(notice_rows(&delivery).await?, 2);
    fixture.objects.close().await?;
    delivery.objects.close().await?;
    Ok(())
}
