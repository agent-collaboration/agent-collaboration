#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Membership commands and poll filters retain each ThreadRef's project qualification.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
mod board_read_setup;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::{create_registered_project, keyed_call, post_at_placement};
use message_board::*;
use serde_json::{Value, json};

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn subscriptions_label_projects_and_qualified_roots_do_not_escape_them()
-> Result<(), TestError> {
    let fixture = BoardFixture::start_poll().await?;
    let (project, _) = fixture.project().await?;
    let other = ProjectId::generate();
    accepted(
        &create_registered_project(
            &fixture,
            json!({"boardId":fixture.board,"projectId":other,"title":"other"}),
        )
        .await?,
    );
    let client = fixture.api.client("qualified membership calls").await?;
    let actor: Identity = serde_json::from_value(fixture.actor.clone())?;
    let mut targets = Vec::new();
    for project_id in [&project, &other] {
        let root = post_at_placement(
            &fixture,
            Placement::Project {
                project_id: project_id.clone(),
            },
            json!({"text":"root"}),
        )
        .await?;
        let thread = ThreadRef {
            thread: ThreadId::from(root),
            project: Some(project_id.clone()),
        };
        let joined = client
            .thread_join(ThreadJoinRequest {
                request_id: RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                thread: thread.clone(),
                actor: actor.clone(),
                role: ParticipantRole::Participant,
                watch: true,
                mode: Some(SubscriptionMode::Poll),
                when_idle: Some(WhenIdle::Hold),
                replace: None,
                note: None,
            })
            .await?;
        assert!(
            matches!(
                joined.result,
                collaboration_client::board::RequestResult::Decided(
                    collaboration_client::board::RequestDecision::Accepted { .. }
                )
            ),
            "{joined:?}"
        );
        let target = SubscriptionTarget::Thread {
            thread: thread.clone(),
        };
        accepted(
            &keyed_call(
                &fixture,
                "thread_subscribe",
                json!({"scope":target,"policy":{"mode":"poll","quietSeconds":0,"capSeconds":0}}),
            )
            .await?,
        );
        targets.push(thread);
    }
    let rows = fixture
        .api
        .call("thread_subscriptions", json!({"actor":fixture.actor}))
        .await?;
    let rows = rows
        .pointer("/result/subscriptions")
        .and_then(Value::as_array)
        .ok_or("subscriptions")?;
    assert_eq!(rows.len(), 2);
    for thread in &targets {
        assert!(
            rows.iter()
                .any(|row| row["projectId"] == json!(thread.project)
                    && row["scope"] == json!({"kind":"thread","threadId":thread.thread})),
            "{rows:?}"
        );
    }
    let first = targets.first().ok_or("first thread")?;
    let second = targets.get(1).ok_or("second thread")?;
    let wrong = ThreadRef {
        thread: first.thread.clone(),
        project: Some(other.clone()),
    };

    let refused = client
        .thread_wait(
            collaboration_protocol::ThreadSubscriptionWaitRequest {
                actor: actor.clone(),
                acting_for: None,
                protocol_version: ProtocolVersion::CURRENT,
                renewals: Vec::new(),
                filter: collaboration_protocol::ThreadSubscriptionWaitFilter::Roots {
                    threads: vec![wrong],
                },
                max_wait_seconds: 0,
            },
            std::time::Duration::from_secs(5),
        )
        .await?;
    assert!(
        matches!(
            refused.outcome,
            collaboration_protocol::ThreadSubscriptionWaitOutcome::Refused {
                reason: BoardError {
                    kind: BoardFailureKind::InvalidField,
                    ..
                }
            }
        ),
        "{refused:?}"
    );
    accepted(
        &keyed_call(
            &fixture,
            "thread_renew",
            json!({"scope":{"kind":"thread","thread":first}}),
        )
        .await?,
    );
    accepted(
        &keyed_call(
            &fixture,
            "thread_unsubscribe",
            json!({"scope":{"kind":"thread","thread":second}}),
        )
        .await?,
    );
    let rows = client
        .thread_subscriptions(collaboration_protocol::ThreadSubscriptionsRequest { actor })
        .await?;
    assert_eq!(rows.subscriptions.len(), 1);
    assert_eq!(
        rows.subscriptions
            .first()
            .ok_or("remaining subscription")?
            .project_id,
        project
    );
    fixture.stop().await
}
