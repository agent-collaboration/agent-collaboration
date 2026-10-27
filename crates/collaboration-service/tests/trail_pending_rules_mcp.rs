//! Reserved pending tags use current workstream authority and explicit blocked-record links.
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
use board_fixture::{BoardFixture, TestError, accepted, refusal_kind};
use message_board::*;
use serde_json::{Value, json};
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove admission; Result propagates fixture failures."
)]
async fn handbacks_require_the_workstream_pair_and_owner_questions_name_a_blocked_record()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let task = fixture.task(&project, &workstream).await?;
    let add = |actor: Value, subject: Value, tag: &str, links: Value| {
        let mut request=fixture.request(&project,json!({"kind":"workTrailAdd","rowId":WorkTrailRowId::generate(),"subject":subject,"text":"recorded pending work","tags":[tag],"links":links}));
        request["actor"] = actor;
        request
    };
    let denied = fixture
        .api
        .call(
            "work_trail_add",
            add(
                fixture.actor.clone(),
                json!({"kind":"task","subject":task}),
                "handback",
                json!([]),
            ),
        )
        .await?;
    assert_eq!(refusal_kind(&denied), Some("roleRequired"), "{denied}");
    let unlinked = fixture
        .api
        .call(
            "work_trail_add",
            add(
                fixture.actor.clone(),
                json!({"kind":"project"}),
                "owner-question",
                json!([]),
            ),
        )
        .await?;
    assert_eq!(refusal_kind(&unlinked), Some("invalidShape"), "{unlinked}");
    assert_eq!(
        unlinked
            .pointer("/result/result/decision/reasons/0/reason/field")
            .or_else(|| unlinked.pointer("/result/result/decision/reasons/0/field")),
        Some(&json!("links"))
    );
    for (revision, role) in [(0, "implementer"), (1, "verifier")] {
        let actor = json!({"kind":"human","humanId":role});
        accepted(&fixture.api.call("role_grant",fixture.request(&project,json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},"role":role,"holder":actor,"model":null,"lineage":null,"expectedRevision":revision,"ownerGrant":null}))).await?);
        accepted(
            &fixture
                .api
                .call(
                    "work_trail_add",
                    add(
                        actor.clone(),
                        json!({"kind":"task","subject":task}),
                        "handback",
                        json!([]),
                    ),
                )
                .await?,
        );
        accepted(
            &fixture
                .api
                .call(
                    "work_trail_add",
                    add(
                        actor,
                        json!({"kind":"workstream","subject":workstream}),
                        "handback",
                        json!([]),
                    ),
                )
                .await?,
        );
    }
    let record_link = serde_json::to_value(EvidencePointer::Record {
        record: task.into(),
    })?;
    accepted(
        &fixture
            .api
            .call(
                "work_trail_add",
                add(
                    fixture.actor.clone(),
                    json!({"kind":"project"}),
                    "owner-question",
                    json!([record_link]),
                ),
            )
            .await?,
    );
    fixture.stop().await
}
