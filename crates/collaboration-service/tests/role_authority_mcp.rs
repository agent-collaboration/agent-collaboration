//! Authority and terminal replay run against real native stores.
#![deny(clippy::too_many_lines)]
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
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn workstream_pair_and_stand_down_acknowledgement_keep_authority_distinct()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let implementer = json!({"kind":"human","humanId":"implementer"});
    let grant = |role: &str, holder: Value, expected: u64| {
        fixture.request(&project,json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},"role":role,"holder":holder,"model":null,"lineage":null,"expectedRevision":expected,"ownerGrant":null}))
    };
    accepted(
        &fixture
            .api
            .call("role_grant", grant("implementer", implementer.clone(), 0))
            .await?,
    );
    let pair = fixture
        .api
        .call("role_grant", grant("verifier", implementer.clone(), 1))
        .await?;
    assert_eq!(refusal_kind(&pair), Some("pairNotDistinct"), "{pair}");
    let mut outsider = grant("verifier", json!({"kind":"human","humanId":"verifier"}), 1);
    outsider["actor"] = implementer;
    let denied = fixture.api.call("role_grant", outsider).await?;
    assert_eq!(refusal_kind(&denied), Some("roleRequired"), "{denied}");
    accepted(
        &fixture
            .api
            .call(
                "role_grant",
                grant("verifier", json!({"kind":"human","humanId":"verifier"}), 1),
            )
            .await?,
    );
    let new_holder = json!({"kind":"human","humanId":"new-lead"});
    let owner_grant = OwnerGrantId::generate();
    accepted(&fixture.api.call("owner_grant_record",fixture.request(&project,json!({"kind":"ownerGrantRecord","grantId":owner_grant,"grant":{"kind":"replaceLead","project":project,"newHolder":new_holder,"reason":"owner relayed replacement"}}))).await?);
    accepted(&fixture.api.call("role_replace_lead",fixture.request(&project,json!({"kind":"roleReplaceLead","newHolder":new_holder,"ownerGrant":owner_grant,"reason":"replace","expectedAssignmentRevision":1}))).await?);
    let stopped=fixture.api.call("work_trail_add",fixture.request(&project,json!({"kind":"workTrailAdd","rowId":WorkTrailRowId::generate(),"subject":{"kind":"project"},"text":"old lead write","tags":[],"links":[]}))).await?;
    assert_eq!(
        refusal_kind(&stopped),
        Some("standDownUnacknowledged"),
        "{stopped}"
    );
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("store")?;
    let read = store.snapshot().await?;
    let head = read.high_water();
    read.finish().await?;
    accepted(
        &fixture
            .api
            .call(
                "role_acknowledge_current",
                fixture.request(
                    &project,
                    json!({"kind":"roleAcknowledgeCurrent","readThrough":head}),
                ),
            )
            .await?,
    );
    accepted(&fixture.api.call("work_trail_add",fixture.request(&project,json!({"kind":"workTrailAdd","rowId":WorkTrailRowId::generate(),"subject":{"kind":"project"},"text":"acknowledged former lead","tags":[],"links":[]}))).await?);
    let no_authority=fixture.api.call("workstream_rename",fixture.request(&project,json!({"kind":"workstreamRename","workstreamId":workstream,"name":"not authorized","expectedRevision":0}))).await?;
    assert_eq!(
        refusal_kind(&no_authority),
        Some("roleRequired"),
        "{no_authority}"
    );
    fixture.stop().await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn lead_release_then_grant_cannot_bypass_the_owner_grant_or_reuse_it() -> Result<(), TestError>
{
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    accepted(&fixture.api.call("role_release",fixture.request(&project,json!({"kind":"roleRelease","subject":{"kind":"project","subject":project},"role":"lead","expectedRevision":1}))).await?);
    let holder = json!({"kind":"human","humanId":"replacement"});
    let grant = |owner_grant: Value, expected: u64| {
        fixture.request(&project,json!({"kind":"roleGrant","subject":{"kind":"project","subject":project},"role":"lead","holder":holder,"model":null,"lineage":null,"expectedRevision":expected,"ownerGrant":owner_grant}))
    };
    let without_owner = fixture
        .api
        .call("role_grant", grant(Value::Null, 2))
        .await?;
    assert_eq!(
        refusal_kind(&without_owner),
        Some("invalidShape"),
        "{without_owner}"
    );
    assert_eq!(
        without_owner.pointer("/result/result/decision/reasons/0/reason/field"),
        Some(&json!("ownerGrant"))
    );
    let owner_grant = OwnerGrantId::generate();
    let recorded = json!(holder);
    accepted(&fixture.api.call("owner_grant_record",fixture.request(&project,json!({"kind":"ownerGrantRecord","grantId":owner_grant,"grant":{"kind":"replaceLead","project":project,"newHolder":recorded,"reason":"explicit owner grant"}}))).await?);
    accepted(
        &fixture
            .api
            .call("role_grant", grant(json!(owner_grant), 2))
            .await?,
    );
    let mut release=fixture.request(&project,json!({"kind":"roleRelease","subject":{"kind":"project","subject":project},"role":"lead","expectedRevision":3}));
    release["actor"] = holder.clone();
    accepted(&fixture.api.call("role_release", release).await?);
    let mut reused = grant(json!(owner_grant), 4);
    reused["actor"] = holder;
    let response = fixture.api.call("role_grant", reused.clone()).await?;
    assert_eq!(refusal_kind(&response), Some("invalidShape"), "{response}");
    assert_eq!(
        response.pointer("/result/result/decision/reasons/0/reason/field"),
        Some(&json!("ownerGrant"))
    );
    assert_eq!(
        fixture.api.call("role_grant", reused).await?["result"]["result"],
        response["result"]["result"]
    );
    fixture.stop().await
}
