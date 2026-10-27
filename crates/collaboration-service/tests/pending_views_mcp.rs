#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Pending and owed views use recorded links, authors and redactions, never prose guesses.
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
use serde_json::{Value, json};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn cards_show_the_current_governing_plan_and_empty_duties_without_inventing_a_role()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let task = fixture.task(&project, &workstream).await?;
    let document = EvidencePointer::Record {
        record: task.clone().into(),
    };
    for (revision, hash) in [(0, "11".repeat(32)), (1, "22".repeat(32))] {
        accepted(&fixture.api.call("project_publish_plan",fixture.request(&project,json!({"kind":"projectPublishPlan","hash":hash,"document":document,"expectedRevision":revision}))).await?);
    }
    let reader = json!({"kind":"human","humanId":"unassigned-reader"});
    let card = fixture
        .api
        .call(
            "view_card",
            json!({"projectId":project,"actor":reader,"subject":{"kind":"task","subject":task}}),
        )
        .await?;
    assert_eq!(card.pointer("/result/assignment"), Some(&Value::Null));
    assert_eq!(card.pointer("/result/pendingHandbacks"), Some(&json!([])));
    assert_eq!(card.pointer("/result/pendingDecisions"), Some(&json!([])));
    assert_eq!(card.pointer("/result/nextObligation"), Some(&Value::Null));
    assert_eq!(
        card.pointer("/result/planRevision"),
        Some(&json!("22".repeat(32)))
    );
    assert_eq!(card.pointer("/result/counts/task"), Some(&json!(1)));
    assert_eq!(card.pointer("/result/counts/planRevision"), Some(&json!(2)));
    assert_eq!(
        card.pointer("/result/sessionState/kind"),
        Some(&json!("notSession"))
    );
    assert_eq!(card.pointer("/result/readMark"), None);
    fixture.stop().await
}

async fn add_trail(
    fixture: &BoardFixture,
    project: &ProjectId,
    actor: Value,
    subject: Value,
    tag: &str,
    links: Vec<RecordRef>,
) -> Result<WorkTrailRowId, TestError> {
    let row = WorkTrailRowId::generate();
    let links: Vec<_> = links
        .into_iter()
        .map(|record| EvidencePointer::Record { record })
        .collect();
    let mut request=fixture.request(project,json!({"kind":"workTrailAdd","rowId":row,"subject":subject,"text":format!("recorded {tag}"),"tags":[tag],"links":links}));
    request
        .as_object_mut()
        .ok_or("request object missing")?
        .insert("actor".to_owned(), actor);
    accepted(&fixture.api.call("work_trail_add", request).await?);
    Ok(row)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn a_lead_with_a_pair_assignment_still_sees_lead_duties() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let implementer = json!({"kind":"human","humanId":"next-lead"});
    let verifier = json!({"kind":"human","humanId":"verifier"});
    for (revision, role, holder) in [
        (0, "implementer", implementer.clone()),
        (1, "verifier", verifier.clone()),
    ] {
        accepted(&fixture.api.call("role_grant", fixture.request(&project, json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},"role":role,"holder":holder,"model":null,"lineage":null,"expectedRevision":revision,"ownerGrant":null}))).await?);
    }
    let subject = json!({"kind":"workstream","subject":workstream});
    let handback = add_trail(
        &fixture,
        &project,
        verifier,
        subject.clone(),
        "handback",
        vec![],
    )
    .await?;
    let new_holder = json!(implementer);
    let grant = OwnerGrantId::generate();
    accepted(&fixture.api.call("owner_grant_record", fixture.request(&project, json!({"kind":"ownerGrantRecord","grantId":grant,"grant":{"kind":"replaceLead","project":project,"newHolder":new_holder,"reason":"owner relayed replacement"}}))).await?);
    accepted(&fixture.api.call("role_replace_lead", fixture.request(&project, json!({"kind":"roleReplaceLead","newHolder":new_holder,"ownerGrant":grant,"reason":"replace","expectedAssignmentRevision":1}))).await?);
    let card = fixture
        .api
        .call(
            "view_card",
            json!({"projectId":project,"actor":implementer,"subject":subject}),
        )
        .await?;
    assert_eq!(
        card.pointer("/result/assignment"),
        Some(&json!({"kind":"lead","subject":project}))
    );
    assert_eq!(
        card.pointer("/result/pendingHandbacks"),
        Some(&json!([RecordRef::from(handback.clone())]))
    );
    assert_eq!(
        card.pointer("/result/nextObligation/subject"),
        Some(&json!(RecordRef::from(handback)))
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn a_lead_decision_stays_settled_after_lead_replacement() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let implementer = json!({"kind":"human","humanId":"implementer"});
    accepted(&fixture.api.call("role_grant",fixture.request(&project,json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},"role":"implementer","holder":implementer,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}))).await?);
    let subject = json!({"kind":"workstream","subject":workstream});
    let handback = add_trail(
        &fixture,
        &project,
        implementer,
        subject.clone(),
        "handback",
        vec![],
    )
    .await?;
    add_trail(
        &fixture,
        &project,
        fixture.actor.clone(),
        subject.clone(),
        "decision",
        vec![handback.into()],
    )
    .await?;
    let new_holder = json!({"kind":"human","humanId":"new-lead"});
    let grant = OwnerGrantId::generate();
    accepted(&fixture.api.call("owner_grant_record",fixture.request(&project,json!({"kind":"ownerGrantRecord","grantId":grant,"grant":{"kind":"replaceLead","project":project,"newHolder":new_holder,"reason":"owner relayed replacement"}}))).await?);
    accepted(&fixture.api.call("role_replace_lead",fixture.request(&project,json!({"kind":"roleReplaceLead","newHolder":new_holder,"ownerGrant":grant,"reason":"replace","expectedAssignmentRevision":1}))).await?);
    let request = json!({"projectId":project,"actor":{"kind":"human","humanId":"new-lead"},"subject":subject});
    let card = fixture.api.call("view_card", request).await?;
    assert_eq!(
        card.pointer("/result/pendingHandbacks"),
        Some(&json!([])),
        "{card}"
    );
    assert_eq!(card.pointer("/result/nextObligation"), Some(&Value::Null));
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions inspect real persisted views and admission results."
)]
async fn pending_views_settle_linked_rows_and_redaction_changes_the_derived_answer()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let task = fixture.task(&project, &workstream).await?;
    let implementer = json!({"kind":"human","humanId":"implementer"});
    let verifier = json!({"kind":"human","humanId":"verifier"});
    for (revision, role, holder) in [
        (0, "implementer", implementer.clone()),
        (1, "verifier", verifier.clone()),
    ] {
        accepted(&fixture.api.call("role_grant",fixture.request(&project,json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},"role":role,"holder":holder,"model":null,"lineage":null,"expectedRevision":revision,"ownerGrant":null}))).await?);
    }
    let subject = json!({"kind":"task","subject":task});
    let handback = add_trail(
        &fixture,
        &project,
        implementer.clone(),
        subject.clone(),
        "handback",
        vec![],
    )
    .await?;
    let other = add_trail(
        &fixture,
        &project,
        verifier.clone(),
        subject.clone(),
        "handback",
        vec![],
    )
    .await?;
    let question = add_trail(
        &fixture,
        &project,
        fixture.actor.clone(),
        subject.clone(),
        "owner-question",
        vec![task.clone().into()],
    )
    .await?;
    let card_request = json!({"projectId":project,"actor":fixture.actor,"subject":subject});
    let card = fixture.api.call("view_card", card_request.clone()).await?;
    let body = card.get("result").ok_or("card has no result")?;
    assert_eq!(
        body.get("pendingHandbacks"),
        Some(&json!([
            RecordRef::from(handback.clone()),
            RecordRef::from(other.clone())
        ]))
    );
    assert_eq!(
        body.get("pendingDecisions"),
        Some(&json!([RecordRef::from(question.clone())]))
    );
    assert_eq!(
        body.pointer("/nextObligation/subject"),
        Some(&json!(RecordRef::from(handback.clone())))
    );
    assert_eq!(
        body.get("assignment"),
        Some(&json!({"kind":"lead","subject":project}))
    );
    assert_eq!(body.get("source"), Some(&json!({"kind":"writer"})));
    for field in ["source", "asOf", "replicatedThrough"] {
        let expected = body.get(field).ok_or("card snapshot context missing")?;
        assert_eq!(
            body.get("nextObligation").and_then(|row| row.get(field)),
            Some(expected),
            "nested obligation lost its snapshot's {field}: {body}"
        );
    }
    assert_eq!(
        body.get("silenceRule"),
        Some(&json!(
            "A claim the board does not contradict is unrecorded, not false."
        ))
    );
    let mut own_request = card_request.clone();
    own_request["actor"] = implementer.clone();
    let own = fixture.api.call("view_card", own_request).await?;
    assert_eq!(
        own.pointer("/result/pendingHandbacks"),
        Some(&json!([RecordRef::from(handback.clone())]))
    );
    assert_eq!(own.pointer("/result/nextObligation"), Some(&Value::Null));
    let owner = fixture.api.call("view_owner", json!({})).await?;
    assert_eq!(
        owner.pointer("/result/waiting/0/subject"),
        Some(&json!(RecordRef::from(question.clone())))
    );
    assert_eq!(
        owner.pointer("/result/waiting/0/blocks"),
        Some(&json!([RecordRef::from(task.clone())]))
    );
    for field in ["source", "asOf", "replicatedThrough"] {
        let expected = body.get(field).ok_or("card snapshot context missing")?;
        assert_eq!(
            owner
                .pointer("/result/waiting/0")
                .and_then(|row| row.get(field)),
            Some(expected),
            "owner row lost its project snapshot's {field}: {owner}"
        );
    }
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project missing")?;
    let mut snapshot = store.snapshot().await?;
    let roles = snapshot.records(RecordKind::RoleAssignment).await?;
    snapshot.finish().await?;
    let lead = roles
        .into_iter()
        .find_map(|record| match record {
            Record::RoleAssignment(row) if row.role() == AuthorityRole::Lead => Some(row.holder),
            _ => None,
        })
        .ok_or("lead missing")?;
    let load_request = json!({"lead":lead});
    let load = fixture
        .api
        .call("view_lead_load", load_request.clone())
        .await?;
    assert_eq!(
        load.pointer("/result/owed")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );
    for owed in load
        .pointer("/result/owed")
        .and_then(Value::as_array)
        .ok_or("owed")?
    {
        for field in ["source", "asOf", "replicatedThrough"] {
            let expected = body.get(field).ok_or("card snapshot context missing")?;
            assert_eq!(
                owed.get(field),
                Some(expected),
                "lead row lost {field}: {owed}"
            );
        }
    }
    // A relayer's decision settles an owner question, but cannot settle a handback to the Lead.
    add_trail(
        &fixture,
        &project,
        implementer.clone(),
        subject.clone(),
        "decision",
        vec![question.clone().into(), handback.clone().into()],
    )
    .await?;
    let still = fixture.api.call("view_card", card_request.clone()).await?;
    assert_eq!(
        still.pointer("/result/pendingHandbacks"),
        body.get("pendingHandbacks")
    );
    assert_eq!(still.pointer("/result/pendingDecisions"), Some(&json!([])));
    assert_eq!(
        fixture
            .api
            .call("view_owner", json!({}))
            .await?
            .pointer("/result/waiting"),
        Some(&json!([]))
    );
    let decision = add_trail(
        &fixture,
        &project,
        fixture.actor.clone(),
        subject.clone(),
        "decision",
        vec![handback.clone().into()],
    )
    .await?;
    let settled = fixture.api.call("view_card", card_request.clone()).await?;
    assert_eq!(
        settled.pointer("/result/pendingHandbacks"),
        Some(&json!([RecordRef::from(other.clone())]))
    );
    accepted(
        &fixture
            .api
            .call(
                "work_trail_redact",
                fixture.request(
                    &project,
                    json!({"kind":"workTrailRedact","rowId":decision,"redacted":true}),
                ),
            )
            .await?,
    );
    let revived = fixture.api.call("view_card", card_request.clone()).await?;
    assert_eq!(
        revived.pointer("/result/pendingHandbacks"),
        body.get("pendingHandbacks")
    );
    accepted(
        &fixture
            .api
            .call(
                "work_trail_redact",
                fixture.request(
                    &project,
                    json!({"kind":"workTrailRedact","rowId":handback,"redacted":true}),
                ),
            )
            .await?,
    );
    accepted(
        &fixture
            .api
            .call(
                "work_trail_redact",
                fixture.request(
                    &project,
                    json!({"kind":"workTrailRedact","rowId":other,"redacted":true}),
                ),
            )
            .await?,
    );
    let empty = fixture.api.call("view_card", card_request).await?;
    assert_eq!(empty.pointer("/result/pendingHandbacks"), Some(&json!([])));
    let recent = empty
        .pointer("/result/recentTrail")
        .and_then(Value::as_array)
        .ok_or("recent trail missing")?;
    assert!(
        recent
            .iter()
            .all(|row| row.get("redacted") == Some(&json!(false)))
    );
    assert!(
        recent
            .iter()
            .all(|row| row.get("rowId") != Some(&json!(decision)))
    );
    accepted(&fixture.api.call("task_set_status",fixture.request(&project,json!({"kind":"taskSetStatus","taskId":task,"status":{"kind":"done"},"expectedRevision":0}))).await?);
    let current = fixture
        .api
        .call("view_work", json!({"projectId":project}))
        .await?;
    let current = current.get("result").ok_or("work view context")?;
    let current_source = current.get("source").ok_or("work view source")?;
    let current_as_of = current.get("asOf").ok_or("work view head")?;
    let current_replicated = current.get("replicatedThrough").ok_or("work view cutoff")?;
    let close_out = fixture
        .api
        .call("view_lead_load", load_request.clone())
        .await?;
    assert_eq!(
        close_out.pointer("/result/owed"),
        Some(
            &json!([{"project":project,"subject":RecordRef::from(workstream.clone()),"obligation":"closeOut","source":current_source,"asOf":current_as_of,"replicatedThrough":current_replicated}])
        )
    );
    accepted(&fixture.api.call("workstream_close",fixture.request(&project,json!({"kind":"workstreamClose","workstreamId":workstream,"reason":"done","note":"closed","expectedRevision":0}))).await?);
    assert_eq!(
        fixture
            .api
            .call("view_lead_load", load_request)
            .await?
            .pointer("/result/owed"),
        Some(&json!([]))
    );
    fixture.stop().await
}
