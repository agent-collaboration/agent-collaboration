//! A real maintenance-pool timeout retries in the same owner and project registry.
use super::super::{SubscriptionClock, subscription_service::OwnerObservation};
use super::owner_fixture::*;
use super::startup_fixture::{exhaust_project_readers, release_project_readers};
use message_board::*;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tracing_subscriber::{Layer, layer::SubscriberExt};

struct PoolTimeoutTrace {
    seen: Arc<AtomicBool>,
    expected: String,
}
impl<TSubscriber: tracing::Subscriber> Layer<TSubscriber> for PoolTimeoutTrace {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _: tracing_subscriber::layer::Context<'_, TSubscriber>,
    ) {
        struct ErrorVisitor<'trace>(&'trace PoolTimeoutTrace);
        impl tracing::field::Visit for ErrorVisitor<'_> {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "error" && format!("{value:?}") == self.0.expected {
                    self.0.seen.store(true, Ordering::SeqCst);
                }
            }
        }
        event.record(&mut ErrorVisitor(self));
    }
}

#[tokio::test(start_paused = true)]
async fn maintenance_timeout_retries_without_quarantining_the_project() {
    let timeout_seen = Arc::new(AtomicBool::new(false));
    let subscriber = tracing_subscriber::registry().with(PoolTimeoutTrace {
        seen: timeout_seen.clone(),
        expected: message_board_storage::ProjectStorageError::Database(sqlx::Error::PoolTimedOut)
            .to_string(),
    });
    let _trace = tracing::subscriber::set_default(subscriber);
    let fixture = Box::pin(OwnerFixture::new()).await;
    let clock = Arc::new(RuntimeTimerClock::new(fixture.clock.clone()));
    let runtime = fixture.native_runtime_with_clock(clock).await;
    runtime
        .observe(|event| matches!(event, OwnerObservation::Sleeping(_)))
        .await;
    // Keep the activity-triggered owner from scanning until its native reader pool
    // is exhausted. No pool, registry or owner is replaced during repair.
    let windows = fixture.store.lock().await;
    let owed = fixture
        .post(&fixture.root, "owed before the failed pass")
        .await;
    let project = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let held = exhaust_project_readers(&project).await;
    timeout_seen.store(false, Ordering::SeqCst);
    drop(windows);
    let retry_deadline = tokio::time::timeout(Duration::from_secs(35), async {
        let mut events = runtime.observations.lock().await;
        loop {
            match events.recv().await.unwrap() {
                OwnerObservation::Sleeping(Some(deadline))
                    if timeout_seen.load(Ordering::SeqCst) =>
                {
                    break Some(deadline);
                }
                OwnerObservation::Stopped => break None,
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert!(
        timeout_seen.load(Ordering::SeqCst),
        "the pass must actually fail with Database(PoolTimedOut)"
    );
    // Recovery includes native worker I/O. Return to elapsed time before its
    // bounded wait rather than auto-advancing that wait past real I/O.
    tokio::time::resume();
    // Release the fault even on the old quarantine path, before checking it.
    release_project_readers(held).await;
    assert!(
        retry_deadline.is_some(),
        "a transient maintenance timeout permanently quarantined this project"
    );

    let own_post = PostId::generate();
    fixture
        .write_as(
            fixture.reader.clone(),
            BoardOperation::ThreadPost {
                post_id: own_post.clone(),
                thread_id: fixture.root.clone(),
                text: PostText::try_from("writable after timeout".to_owned()).unwrap(),
                references: Vec::new(),
                addressees: Vec::new(),
            },
            fixture.clock.now(),
        )
        .await;
    let application =
        (crate::ApplicationFixture::new(SERVICE_ID, "00000000-0000-4000-8000-000000000002")
            .unwrap()
            .with_project_admission(fixture.admission.clone()))
        .build();
    let shown = application
        .board()
        .post_show(PostShowRequest {
            post_id: own_post.clone(),
        })
        .await
        .unwrap()
        .0;
    assert_eq!(shown.post_id, own_post);
    assert_eq!(shown.project_id, fixture.project);
    assert_eq!(shown.text.as_str(), "writable after timeout");
    let request = tokio::time::timeout(Duration::from_secs(5), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await
    .unwrap();
    let notice = fixture
        .interactions
        .get_push_record(&request.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    let activity = notice.activity.unwrap();
    assert_eq!(
        activity.ranges,
        vec![collaboration_protocol::PushActivityRange {
            thread_id: fixture.root.clone(),
            project_id: fixture.project.clone(),
            from: owed,
            through: owed,
        }]
    );
    runtime.await_settled().await;
    runtime.owner_barrier(&fixture.reader).await;
    assert!(
        runtime.requests.lock().await.try_recv().is_err(),
        "owed range must be delivered once"
    );
    runtime.close().await;
}
