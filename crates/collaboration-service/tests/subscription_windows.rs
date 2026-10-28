#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
pub mod board_fixture;
use collaboration_service::SubscriptionWindowStore;
use message_board::*;
use std::sync::Arc;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Native settlement must leave both membership and feed unchanged."
)]
async fn local_selection_settles_without_changing_replicated_membership_or_feed() -> TestResult {
    let fixture = board_fixture::BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let owner = Identity::Human {
        human_id: HumanId::try_from("reader".to_owned())?,
    };
    let subscribe: ProjectRequest = serde_json::from_value(serde_json::json!({
        "requestId":RequestId::generate(),"projectId":project,"protocolVersion":1,
        "actingFor":null,"actor":owner,
        "operation":{"kind":"threadSubscribe","scope":{"kind":"project","projectId":project},
            "policy":{"mode":"poll","quietSeconds":0,"capSeconds":0}},
    }))?;
    let subscribed = fixture._admission.submit(&subscribe).await?;
    assert!(matches!(
        subscribed.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    let store = fixture._registry.open_project(&project).await?;
    let mut read = store.snapshot().await?;
    let membership = read
        .records(RecordKind::SubscriptionMembership)
        .await?
        .into_iter()
        .find_map(|record| match record {
            Record::SubscriptionMembership(membership) if membership.reader() == &owner => {
                Some(membership)
            }
            _ => None,
        })
        .ok_or("project membership")?;
    read.finish().await?;
    let membership_id = membership.envelope.id.clone();
    let activated_at = membership.activated_at();
    let membership_reference = RecordRef {
        kind: RecordKind::SubscriptionMembership,
        id: membership_id.clone(),
    };
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(
            &fixture._directory.path().join("local-window-proof.sqlite"),
        )
        .await?,
    );
    let mut windows = SubscriptionWindowStore::new(fixture._registry.clone(), interactions.clone());
    windows
        .rescan_missing_subscription_windows(&owner, chrono::Utc::now())
        .await?;
    let post_id = PostId::generate();
    let thread_id = ThreadId::from(post_id.clone());
    // The ordinary post path owns covered-watch materialization; no fixture copy of its effect.
    let posted = fixture
        ._admission
        .submit(&serde_json::from_value(fixture.request(
            &project,
            serde_json::json!({"kind":"threadCreate","postId":post_id,
            "about":{"kind":"project","subject":{"line":"Fixture discussion"}},
            "text":"new work","references":[],"participation":null,"watch":false,"addressees":[]}),
        ))?)
        .await?;
    assert!(matches!(
        posted.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    let now = chrono::Utc::now();
    windows
        .rescan_missing_subscription_windows(&owner, now)
        .await?;
    let mut before = store.snapshot().await?;
    let high = before.high_water();
    let before_membership = before.record(&membership_reference).await?;
    before.finish().await?;
    assert_eq!(
        windows.due_subscription_roots(&owner, now).await?,
        vec![thread_id.clone()]
    );
    let (batch, settlement) = windows
        .select_subscription_notice(&owner, &[thread_id], now, usize::MAX)
        .await?;
    assert_eq!(batch.roots.len(), 1);
    assert_eq!(batch.roots[0].post_count, 1);
    assert!(
        windows
            .due_subscription_roots(&owner, now)
            .await?
            .is_empty()
    );
    windows
        .settle_subscription_batch(&owner, &settlement, SubscriptionDeliveryOutcome::Accepted)
        .await?;
    let mut after = store.snapshot().await?;
    assert_eq!(after.high_water(), high);
    assert_eq!(
        after.record(&membership_reference).await?,
        before_membership
    );
    after.finish().await?;
    let progress = interactions
        .subscription_progress(&project, &membership_id, activated_at)
        .await?
        .ok_or("local progress missing")?;
    assert!(progress.roots().iter().all(|root| root.window.is_none()));
    assert_eq!(
        progress.last_outcome(),
        Some(&SubscriptionDeliveryOutcome::Accepted)
    );
    drop(windows);
    drop(store);
    fixture.stop().await
}
