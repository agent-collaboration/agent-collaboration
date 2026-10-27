#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Reader state has no activity position, and summaries never acknowledge content.
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
use board_fixture::{BoardFixture, TestError};
use message_board::*;
use serde_json::json;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn consecutive_content_reads_and_keyed_acknowledgements_never_feed_a_read_mark()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let read = store.snapshot().await?;
    let before = read.high_water();
    read.finish().await?;
    let reader = Identity::Human {
        human_id: HumanId::try_from("content reader".to_owned())?,
    };
    let request = json!({"projectId":project,"reader":{"kind":"identified","reader":reader},"after":{"epoch":1,"position":0},"limit":100,"includeRedacted":true});
    let first = fixture.api.call("feed_read", request.clone()).await?;
    assert_eq!(
        first.pointer("/result/readMark/kind"),
        Some(&json!("advanced")),
        "{first}"
    );
    let second = fixture.api.call("feed_read", request).await?;
    assert_eq!(
        second.pointer("/result/readMark/kind"),
        Some(&json!("unchanged"))
    );
    let mut read = store.snapshot().await?;
    assert_eq!(read.high_water(), before, "two reads create no feed record");
    let mark = read
        .read_mark(
            &reader,
            &ReadScope::Project {
                project_id: project.clone(),
            },
        )
        .await?
        .ok_or("mark")?;
    assert_eq!(
        serde_json::to_value(mark.envelope.position)?,
        json!({"kind":"unsequenced"}),
        "reader state is not assigned a borrowed activity position"
    );
    read.finish().await?;
    let acknowledgement = fixture.api.call("inbox_acknowledge", json!({"protocolVersion":1,"actor":{"kind":"human","humanId":"ack reader"},"actingFor":null,"entries":[{"requestId":RequestId::generate(),"scope":{"kind":"project","projectId":project},"through":before}]})).await?;
    assert_eq!(
        acknowledgement.pointer("/result/outcomes/0/outcome/reply/result/decision/effects/entries"),
        Some(&json!([])),
        "{acknowledgement}"
    );
    assert_eq!(
        acknowledgement
            .pointer("/result/outcomes/0/outcome/reply/result/decision/effects/totalCount"),
        Some(&json!(0))
    );
    let mut read = store.snapshot().await?;
    let FeedRead::Entries { entries, .. } = read.feed_after(before, 100, true).await? else {
        return Err("epoch".into());
    };
    assert_eq!(
        entries.len(),
        1,
        "only a keyed acknowledgement's actual ledger decision is activity"
    );
    assert!(entries.iter().all(|entry| matches!(&entry.content, FeedContent::Shown(record) if matches!(record.as_ref(), Record::Request(_)))));
    read.finish().await?;
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn a_continuation_card_does_not_mark_its_subject_actor() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let owner: Identity = serde_json::from_value(fixture.actor.clone())?;
    let card = fixture
        .api
        .call(
            "view_card",
            json!({"projectId":project,"actor":owner,"subject":{"kind":"project"}}),
        )
        .await?;
    assert!(card.get("result").is_some(), "{card}");
    assert!(
        card.pointer("/result/readMark").is_none(),
        "a summary carries no mark result"
    );
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut snapshot = store.snapshot().await?;
    let before = snapshot.high_water();
    let lead = snapshot
        .records(RecordKind::RoleAssignment)
        .await?
        .into_iter()
        .find_map(|record| match record {
            Record::RoleAssignment(role) if role.role() == AuthorityRole::Lead => Some(role.holder),
            _ => None,
        })
        .ok_or("lead")?;
    snapshot.finish().await?;
    for (route, request) in [
        ("view_work", json!({"projectId":project})),
        ("view_owner", json!({})),
        ("view_lead_load", json!({"lead":lead})),
    ] {
        let view = fixture.api.call(route, request).await?;
        assert!(view.get("result").is_some(), "{route}: {view}");
        assert!(
            view.pointer("/result/readMark").is_none()
                && view.pointer("/result/readMarks").is_none(),
            "{route}: {view}"
        );
    }
    let mut read = store.snapshot().await?;
    assert!(
        read.read_mark(
            &owner,
            &ReadScope::Project {
                project_id: project
            }
        )
        .await?
        .is_none(),
        "the card's subject actor has read no content"
    );
    assert_eq!(read.high_water(), before);
    assert!(read.records(RecordKind::ReadMark).await?.is_empty());
    read.finish().await?;
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn a_persisted_not_writer_refusal_hides_reads_without_inventing_a_successor()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let read = store.snapshot().await?;
    let before = read.high_water();
    read.finish().await?;
    use runtime_primitives::Clock;
    fixture
        ._registry
        .database(&project)
        .await?
        .observe_renewal(
            fleet_sync_protocol::RenewAnswer::Refused(
                fleet_sync_protocol::LeaseRefusal::NotWriter { current: None },
            ),
            runtime_primitives::SystemClock.now(),
        )
        .await?;
    // Native diagnostic access retains the actual unknown authority; the
    // public unavailable response must not invent a current epoch.
    assert!(matches!(
        store.begin_admission(fixture.record_origin()?).await,
        Err(message_board_storage::ProjectStorageError::WriteRefused(
            fleet_sync_writer::WriteRefusal::NotWriter
        ))
    ));
    let write = fixture.api.call("work_trail_add", fixture.request(&project, json!({
        "kind":"workTrailAdd","rowId":WorkTrailRowId::generate(),"subject":{"kind":"project"},
        "text":"unanswered non-writer request","tags":[],"links":[]
    }))).await?;
    assert_eq!(
        write.pointer("/error/data/kind"),
        Some(&json!("boardUnavailable")),
        "{write}"
    );
    let response = fixture.api.call("feed_read", json!({"projectId":project,"reader":{"kind":"identified","reader":fixture.actor},"after":{"epoch":1,"position":0},"limit":100,"includeRedacted":true})).await?;
    assert_eq!(
        response.pointer("/error/data/kind"),
        Some(&json!("boardUnavailable")),
        "{response}"
    );
    assert!(response.get("result").is_none(), "{response}");
    let mut read = store.snapshot().await?;
    assert_eq!(read.high_water(), before);
    assert!(read.records(RecordKind::ReadMark).await?.is_empty());
    read.finish().await?;
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn anonymous_content_reads_have_no_mark_outcome_and_empty_threads_mark_nothing()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let response = fixture.api.call("feed_read", json!({"projectId":project,"reader":{"kind":"anonymous"},"after":{"epoch":1,"position":0},"limit":100,"includeRedacted":true})).await?;
    assert_eq!(
        response.pointer("/result/readMark"),
        Some(&json!(null)),
        "{response}"
    );
    let root = PostId::generate();
    let mut create = fixture.header();
    create.as_object_mut().ok_or("header")?.extend(json!({"projectId":project,"postId":root,"text":"root","references":[],"addressees":[],"about":{"kind":"project","subject":{"line":"Fixture discussion"}},"role":null,"watch":false}).as_object().ok_or("fields")?.clone());
    board_fixture::accepted(&fixture.api.call("thread_create", create).await?);
    let thread = ThreadId::try_from(root.as_str().to_owned())?;
    let reader = Identity::Human {
        human_id: HumanId::try_from("empty thread reader".to_owned())?,
    };
    let response = fixture
        .api
        .call(
            "thread_show",
            json!({"thread":{"thread":thread},"reader":{"kind":"identified","reader":reader}}),
        )
        .await?;
    assert_eq!(
        response.pointer("/result/readMark/kind"),
        Some(&json!("unchanged")),
        "{response}"
    );
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    assert!(
        read.read_mark(&reader, &ReadScope::Thread { thread_id: thread })
            .await?
            .is_none()
    );
    read.finish().await?;
    fixture.stop().await
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Boundary assertions reject retired wire variants."
)]
fn read_mark_failures_are_the_closed_write_gate_family() -> Result<(), TestError> {
    for reason in [
        json!({"kind":"overloaded"}),
        json!({"kind":"standDownUnacknowledged"}),
        json!({"kind":"notWriter","current":null}),
    ] {
        let _: ReadMarkOutcome =
            serde_json::from_value(json!({"kind":"notAdvanced","reason":reason}))?;
    }
    for reason in ["unidentified", "incompleteRange", "invalidCoverage"] {
        assert!(
            serde_json::from_value::<ReadMarkOutcome>(
                json!({"kind":"notAdvanced","reason":{"kind":reason}})
            )
            .is_err()
        );
    }
    Ok(())
}
