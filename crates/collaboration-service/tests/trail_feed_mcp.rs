//! V2's exact native positions are read through the real MCP entry point.
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
use board_fixture::BoardFixture;
use message_board::*;
use serde_json::{Value, json};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn redacted_41_and_shown_42_keep_feed_coverage_and_descending_register_bounds()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let fixture = BoardFixture::start().await?;
    let api = &fixture.api;
    let project = ProjectId::generate();
    let workspace = fixture.workspace.clone();
    let board = fixture.board.clone();
    let input: ProjectCreateRequest = serde_json::from_value(json!({
        "requestId":RequestId::generate(),"protocolVersion":1,"actor":fixture.actor,
        "projectId":project,"boardId":board,"title":project.as_str()
    }))?;
    fixture.prepare_registered_creation(&input).await?;
    // Native fixture setup controls exact positions, independently of the read implementation.
    let store = fixture._registry.open_project(&project).await?;
    let actor = Identity::Human {
        human_id: HumanId::try_from("reader".to_owned())?,
    };
    let cursor = FeedCursor {
        epoch: EpochNumber::try_from(1)?,
        position: FeedPosition::ZERO,
    };
    let origin = fixture.record_origin()?;
    let envelope = |id| -> Result<RecordEnvelope, Box<dyn std::error::Error + Send + Sync>> {
        Ok(RecordEnvelope {
            id,
            scope: RecordScope {
                workspace: workspace.clone(),
                board: Some(board.clone()),
                project: Some(project.clone()),
            },
            actor: RecordedActor::declared(actor.clone()),
            request: None,
            position: RecordPlacement::Project(ProjectFeedEntry {
                cursor,
                origin: origin.clone(),
            }),
            time: RecordTime::Router("2026-10-12T00:00:00Z".parse()?),
        })
    };
    let mut ids = Vec::new();
    let mut write = store.begin_admission(origin.clone()).await?;
    write
        .append_record(Record::Project(ProjectState {
            envelope: envelope(project.clone().into())?,
            project_id: project.clone(),
            revision: SubjectRevision::INITIAL,
            closed_at: None,
            description: Description::try_from(String::new())?,
        }))
        .await?;
    for position in 2..=42 {
        let row_id = WorkTrailRowId::generate();
        let row = WorkTrailRow {
            envelope: envelope(row_id.clone().into())?,
            row_id: row_id.clone(),
            project_id: project.clone(),
            subject: TrailSubject::Project,
            text: TrailText::try_from(format!("row {position}"))?,
            tags: if position >= 41 {
                vec![TrailTag::try_from("decision".to_owned())?]
            } else {
                Vec::new()
            },
            links: Vec::new(),
            redacted: false,
        };
        assert_eq!(
            write
                .append_record(Record::WorkTrailRow(row))
                .await?
                .position
                .get(),
            position
        );
        ids.push(row_id);
    }
    write
        .append_record(Record::TrailRedaction(TrailRedaction {
            envelope: envelope(RecordId::generate())?,
            row: ids[39].clone(),
            redacted: true,
        }))
        .await?;
    write.commit().await?;
    let feed = api.call("feed_read",json!({"projectId":project,"after":{"epoch":1,"position":40},"limit":2,"includeRedacted":false,"reader":{"kind":"anonymous"}})).await?;
    assert_eq!(
        feed.pointer("/result/value/coveredThrough/position"),
        Some(&json!(42)),
        "{feed}"
    );
    assert_eq!(
        feed.pointer("/result/value/entries/0/content/kind"),
        Some(&json!("redacted")),
        "{feed}"
    );
    assert_eq!(
        feed.pointer("/result/value/entries/1/position"),
        Some(&json!(42)),
        "{feed}"
    );
    let empty = api.call("work_trail_show",json!({"projectId":project,"selection":{"kind":"forward","after":{"epoch":1,"position":40}},"limit":2,"subject":null,"tags":["excluded"],"includeRedacted":false,"reader":{"kind":"anonymous"}})).await?;
    assert_eq!(
        empty.pointer("/result/value/page/rows"),
        Some(&json!([])),
        "{empty}"
    );
    assert_eq!(
        empty.pointer("/result/value/page/coveredThrough/position"),
        Some(&json!(42)),
        "{empty}"
    );
    let first_request = json!({"projectId":project,"selection":{"kind":"decisions","after":null,"before":null,"rangeTop":null},"limit":1,"subject":null,"tags":[],"includeRedacted":true,"reader":{"kind":"anonymous"}});
    let first = api.call("work_trail_show", first_request.clone()).await?;
    assert_eq!(
        first
            .pointer("/result/value/page/rows/0/rowId")
            .and_then(Value::as_str),
        Some(ids[40].as_str()),
        "{first}"
    );
    assert!(
        first
            .pointer("/result/value/page/coveredThrough")
            .is_some_and(Value::is_null)
    );
    let mut continuation = first_request;
    continuation["selection"]["before"] = first
        .pointer("/result/value/page/olderThan")
        .ok_or("olderThan missing")?
        .clone();
    continuation["selection"]["rangeTop"] = first
        .pointer("/result/value/page/rangeTop")
        .ok_or("rangeTop missing")?
        .clone();
    let second = api.call("work_trail_show", continuation).await?;
    assert_eq!(
        second
            .pointer("/result/value/page/rows/0/rowId")
            .and_then(Value::as_str),
        Some(ids[39].as_str()),
        "{second}"
    );
    assert_eq!(
        second.pointer("/result/value/page/rows/0/redacted"),
        Some(&json!(true)),
        "{second}"
    );
    assert_eq!(
        second.pointer("/result/value/page/coveredThrough/position"),
        Some(&json!(43)),
        "{second}"
    );
    assert!(
        second
            .pointer("/result/value/page/olderThan")
            .is_some_and(Value::is_null)
    );
    fixture.stop().await?;
    Ok(())
}
