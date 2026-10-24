//! V1/V2 compose authority, guard facts, moves and terminal replay on real native stores.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted, refusal_kind};
use message_board::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn guarded_moves_record_composed_refusals_and_replay_without_duplicate_intents()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let source=json!({"initialStatus":"draft","edges":[{"name":"finish","from":"draft","to":"done","requiredRole":"lead","guards":[{"name":"tests","evaluator":{"kind":"human","humanId":"evaluator"}},{"name":"review","evaluator":{"kind":"human","humanId":"evaluator"}}]}]}).to_string();
    let mut register = fixture.header();
    register["workspaceId"] = json!(fixture.workspace);
    register["name"] = json!("flow");
    register["version"] = json!("v1");
    register["source"] = json!(source);
    accepted(&fixture.api.call("process_register", register).await?);
    let definition = json!({"name":"flow","version":"v1","sourceHash":format!("{:x}",Sha256::digest(source.as_bytes()))});
    accepted(&fixture.api.call("project_pin_definition",fixture.request(&project,json!({"kind":"projectPinDefinition","definition":definition,"expectedRevision":0}))).await?);
    let evaluator = json!({"kind":"human","humanId":"evaluator"});
    for (guard, outcome) in [("tests", "unmet"), ("review", "unavailable")] {
        accepted(&fixture.api.call("fact_record",fixture.request(&project,json!({"kind":"factRecord","fact":{"factId":FactId::generate(),"kind":{"kind":"evaluatorResult","fact":{"guard":guard,"subject":{"kind":"project","id":project},"outcome":outcome,"evidence":[],"evaluator":{"actor":evaluator,"engine":null,"calibration":null},"executor":null}},"links":[{"kind":"project","id":project}],"producerTime":"2026-10-12T00:00:00Z","anchor":null}}))).await?);
    }
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("store")?;
    let read = store.snapshot().await?;
    let check_head = read.high_water();
    read.finish().await?;
    let checked = fixture.api.call("move_check", json!({"projectId":project,"actor":fixture.actor,"moveName":"finish","expectedRevision":1})).await?;
    assert_eq!(
        checked.pointer("/result/kind"),
        Some(&json!("rejected")),
        "{checked}"
    );
    assert_eq!(
        checked
            .pointer("/result/reasons")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );
    let read = store.snapshot().await?;
    assert_eq!(read.high_water(), check_head);
    read.finish().await?;
    let movement = MoveId::generate();
    let rejected_request=fixture.request(&project,json!({"kind":"moveRequest","moveId":movement,"moveName":"finish","evidence":[],"reason":"attempt before evidence","expectedRevision":1,"addressees":[]}));
    let rejected = fixture
        .api
        .call("move_request", rejected_request.clone())
        .await?;
    let reasons = rejected
        .pointer("/result/result/decision/reasons")
        .and_then(Value::as_array)
        .ok_or("move refusals")?;
    assert_eq!(reasons.len(), 2, "{rejected}");
    assert_eq!(
        reasons
            .iter()
            .filter_map(|reason| reason.pointer("/reason/kind").and_then(Value::as_str))
            .collect::<Vec<_>>(),
        vec!["guardUnmet", "guardUnavailable"],
        "{rejected}"
    );
    assert_eq!(
        fixture
            .api
            .call("move_request", rejected_request)
            .await?
            .pointer("/result/result"),
        rejected.pointer("/result/result")
    );
    let intents = fixture
        .api
        .call("intent_list", json!({"projectId":project,"page":{}}))
        .await?;
    let rejected_intents = intents
        .pointer("/result/records")
        .and_then(Value::as_array)
        .ok_or("intents")?
        .iter()
        .filter(|record| record.pointer("/record/cause/id") == Some(&json!(movement)))
        .count();
    assert_eq!(rejected_intents, 1, "{intents}");
    let repeated_move = MoveId::generate();
    let repeated = fixture.api.call("move_request", fixture.request(&project, json!({"kind":"moveRequest","moveId":repeated_move,"moveName":"finish","evidence":[],"reason":"repeat the rejected request","expectedRevision":1,"addressees":[]}))).await?;
    assert_eq!(
        repeated.pointer("/result/result/decision/reasons"),
        rejected.pointer("/result/result/decision/reasons")
    );
    let intents = fixture
        .api
        .call("intent_list", json!({"projectId":project,"page":{}}))
        .await?;
    assert!(
        !intents
            .pointer("/result/records")
            .and_then(Value::as_array)
            .ok_or("intents")?
            .iter()
            .any(|record| record.pointer("/record/cause/id") == Some(&json!(repeated_move))),
        "{intents}"
    );
    let mut read = store.snapshot().await?;
    assert!(read.record(&repeated_move.into()).await?.is_some());
    read.finish().await?;

    let evaluator = json!({"kind":"human","humanId":"evaluator"});
    for guard in ["tests", "review"] {
        accepted(&fixture.api.call("fact_record",fixture.request(&project,json!({"kind":"factRecord","fact":{"factId":FactId::generate(),"kind":{"kind":"evaluatorResult","fact":{"guard":guard,"subject":{"kind":"project","id":project},"outcome":"met","evidence":[],"evaluator":{"actor":evaluator,"engine":null,"calibration":null},"executor":null}},"links":[{"kind":"project","id":project}],"producerTime":"2026-10-12T00:00:00Z","anchor":null}}))).await?);
    }
    let read = store.snapshot().await?;
    let allowed_head = read.high_water();
    read.finish().await?;
    let checked = fixture.api.call("move_check", json!({"projectId":project,"actor":fixture.actor,"moveName":"finish","expectedRevision":1})).await?;
    assert_eq!(
        checked.pointer("/result/kind"),
        Some(&json!("allowed")),
        "{checked}"
    );
    let read = store.snapshot().await?;
    assert_eq!(read.high_water(), allowed_head);
    read.finish().await?;
    let successful=fixture.request(&project,json!({"kind":"moveRequest","moveId":MoveId::generate(),"moveName":"finish","evidence":[],"reason":"evidence recorded","expectedRevision":1,"addressees":[{"kind":"role","recipient":{"kind":"lead","subject":project}}]}));
    let decision = fixture.api.call("move_request", successful.clone()).await?;
    accepted(&decision);
    assert_eq!(
        fixture
            .api
            .call("move_request", successful)
            .await?
            .pointer("/result/result"),
        decision.pointer("/result/result")
    );
    let stale=fixture.api.call("move_request",fixture.request(&project,json!({"kind":"moveRequest","moveId":MoveId::generate(),"moveName":"finish","evidence":[],"reason":"stale view","expectedRevision":1,"addressees":[]}))).await?;
    assert_eq!(refusal_kind(&stale), Some("staleRevision"), "{stale}");
    fixture.stop().await?;
    Ok(())
}
#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
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
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
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

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn move_check_uses_recorded_state_and_leaves_no_ledger_feed_or_intent()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("store")?;
    let read = store.snapshot().await?;
    let before = read.high_water();
    read.finish().await?;
    let response=fixture.api.call("move_check",json!({"projectId":project,"actor":fixture.actor,"moveName":"missing","expectedRevision":0})).await?;
    assert_eq!(
        response.pointer("/result/kind"),
        Some(&json!("rejected")),
        "{response}"
    );
    assert_eq!(
        response.pointer("/result/reasons/0/kind"),
        Some(&json!("moveNotAllowed"))
    );
    let read = store.snapshot().await?;
    assert_eq!(read.high_water(), before);
    read.finish().await?;
    fixture.stop().await
}
