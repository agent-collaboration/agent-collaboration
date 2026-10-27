#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Optional cause/thread/trail joins cannot discard unrelated intents after reopen.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use serde_json::json;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn reopened_cards_keep_project_intents_and_only_the_relevant_subject_intents()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let task = fixture.task(&project, &workstream).await?;
    let trail = WorkTrailRowId::generate();
    accepted(&fixture.api.call("work_trail_add", fixture.request(&project, json!({"kind":"workTrailAdd","rowId":trail,"subject":{"kind":"task","subject":task},"text":"intent trail subject","tags":[],"links":[]}))).await?);
    let mut relevant_posts = Vec::new();
    let mut irrelevant_post = None;
    let unrelated_subject = fixture.create_discussion_subject(&project).await?;
    for (about, relevant) in [
        (ThreadSubject::Task(task.clone()), true),
        (ThreadSubject::WorkTrailRow(trail.clone()), true),
        (unrelated_subject, false),
    ] {
        let post = PostId::generate();
        if relevant {
            relevant_posts.push(RecordRef::from(post.clone()));
        } else {
            irrelevant_post = Some(RecordRef::from(post.clone()));
        }
        let mut request = fixture.header();
        request.as_object_mut().ok_or("request")?.extend(json!({"actor":{"kind":"human","humanId":post},"projectId":project,"postId":post,"about":about,"text":"addressed post","references":[],"role":null,"watch":false,"addressees":[{"kind":"actor","recipient":{"kind":"human","humanId":"recipient"}}]}).as_object().ok_or("fields")?.clone());
        accepted(&fixture.api.call("thread_create", request).await?);
    }
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let intents = read.records(RecordKind::NotifyIntent).await?;
    let expected: Vec<_> = intents
        .iter()
        .filter_map(|record| match record {
            Record::NotifyIntent(intent) if relevant_posts.contains(&intent.cause) => {
                Some(record.reference())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        expected.len(),
        2,
        "both explicit post targets create durable intents"
    );
    assert!(intents.iter().any(|record| matches!(record, Record::NotifyIntent(intent) if Some(&intent.cause) == irrelevant_post.as_ref())));
    let project_expected: Vec<_> = intents.iter().map(Record::reference).collect();
    read.finish().await?;
    drop(store);
    let fixture = fixture.restart_read_fixture().await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("reopened project")?;
    let mut read = store.snapshot().await?;
    for subject in [
        TrailSubject::Task(task),
        TrailSubject::Workstream(workstream),
    ] {
        assert_eq!(
            read.card_intent_page_keys(&subject, "")
                .await?
                .into_iter()
                .collect::<Vec<_>>(),
            expected,
            "{subject:?}"
        );
    }
    assert_eq!(
        read.card_intent_page_keys(&TrailSubject::Project, "")
            .await?
            .into_iter()
            .collect::<Vec<_>>(),
        project_expected
    );
    read.finish().await?;
    fixture.stop().await
}
