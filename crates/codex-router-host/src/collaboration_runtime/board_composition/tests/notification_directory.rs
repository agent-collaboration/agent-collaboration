//! Actual Host session lookup, native claim, cache reuse and typed Unknown refusal.
use super::notification_owner::ProjectNotificationFixture;
use super::*;
use board_sync_protocol::InstallationPeerLookup;
use message_board::*;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn a_live_installation_lookup_freezes_its_peer_outside_the_project_lock_and_then_uses_the_cache()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let mut fixture = Box::pin(ProjectNotificationFixture::open(directory.path())).await?;
    let service: ServiceId = "00000000-0000-4000-8000-000000000111"
        .to_owned()
        .try_into()?;
    let peer = fleet_sync_protocol::PeerId::new(durable_steps::Uuid::now_v7())?;
    fixture
        .harness
        .central
        .directory
        .answer(service.clone(), InstallationPeerLookup::Enrolled { peer })
        .await;
    let (entered, release) = fixture.harness.central.directory.hold_next().await;
    let post = fixture
        .post("live-directory-first", Some(service.clone()))
        .await?;
    fixture.start().await?;
    tokio::time::timeout(BOUND, entered).await??;
    assert!(matches!(
        fixture.intent_for(&post).await?.state(),
        IntentState::Emitted {}
    ));
    assert!(
        fixture
            .harness
            .mail
            .owner
            .installation_peer(&service)
            .await?
            .is_none()
    );
    // A held network response must not hold the admission lock.
    tokio::time::timeout(
        Duration::from_secs(5),
        fixture.post("live-directory-concurrent", None),
    )
    .await
    .map_err(|_| io::Error::other("directory network held the project admission lock"))??;
    release
        .send(())
        .map_err(|_| io::Error::other("live lookup caller closed"))?;
    let first = fixture
        .wait_intent(&post, |state| {
            matches!(state, IntentState::HandedOff { .. })
        })
        .await?;
    assert_eq!(fixture.frozen_peer(&first).await?, peer);
    assert_eq!(
        fixture
            .harness
            .mail
            .owner
            .installation_peer(&service)
            .await?
            .ok_or("binding was not cached")?
            .as_str(),
        peer.as_uuid().to_string()
    );
    let next = fixture
        .post("live-directory-second", Some(service.clone()))
        .await?;
    fixture
        .publisher
        .queue_project(fixture.project.clone())
        .await?;
    let second = fixture
        .wait_intent(&next, |state| {
            matches!(state, IntentState::HandedOff { .. })
        })
        .await?;
    assert_eq!(fixture.frozen_peer(&second).await?, peer);
    assert_eq!(
        fixture.harness.central.directory.read_count(&service).await,
        1,
        "cache replay performed another live lookup"
    );
    fixture.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn an_unknown_installation_lookup_refuses_the_actual_intent_without_freezing_mail()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let mut fixture = Box::pin(ProjectNotificationFixture::open(directory.path())).await?;
    let service: ServiceId = "00000000-0000-4000-8000-000000000112"
        .to_owned()
        .try_into()?;
    fixture
        .harness
        .central
        .directory
        .answer(service.clone(), InstallationPeerLookup::Unknown)
        .await;
    let post = fixture
        .post("live-directory-unknown", Some(service.clone()))
        .await?;
    fixture.start().await?;
    let result = fixture
        .wait_intent(&post, |state| {
            matches!(
                state,
                IntentState::Refused {
                    reason: IntentRefusal::RecipientUnknown {}
                }
            )
        })
        .await;
    let reads = fixture.harness.central.directory.read_count(&service).await;
    let cached = fixture
        .harness
        .mail
        .owner
        .installation_peer(&service)
        .await?;
    if let Ok(intent) = &result {
        assert!(matches!(
            intent.state(),
            IntentState::Refused {
                reason: IntentRefusal::RecipientUnknown {}
            }
        ));
    }
    let store = fixture
        .harness
        .registry
        .existing_project(&fixture.project)
        .await?
        .ok_or("project file")?;
    let mut read = store.snapshot().await?;
    let handoffs = read.mail_handoff_page(HandoffSeq::ZERO, 100).await?;
    read.finish().await?;
    drop(store);
    fixture.close().await?;
    assert!(
        result.is_ok(),
        "Unknown did not become the typed recipientUnknown refusal"
    );
    assert_eq!(reads, 1);
    assert!(cached.is_none());
    assert_eq!(
        handoffs.len(),
        1,
        "Unknown allocated a second handoff beyond the original seed"
    );
    Ok(())
}
