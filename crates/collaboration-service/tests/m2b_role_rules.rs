#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! U64: authority, distinctness and cardinality through real MCP admission and Turso.
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

fn person(name: &str) -> Value {
    json!({"kind":"human","humanId":name})
}
fn grant(
    fixture: &BoardFixture,
    project: &ProjectId,
    subject: Value,
    role: &str,
    holder: Value,
    revision: u64,
) -> Value {
    fixture.request(project, json!({"kind":"roleGrant","subject":subject,"role":role,"holder":holder,"model":null,"lineage":null,"expectedRevision":revision,"ownerGrant":null}))
}
async fn assignments(
    fixture: &BoardFixture,
    project: &ProjectId,
) -> Result<Vec<RoleAssignment>, TestError> {
    let store = fixture
        ._registry
        .existing_project(project)
        .await?
        .ok_or("project store")?;
    let mut snapshot = store.snapshot().await?;
    let rows = snapshot
        .records(RecordKind::RoleAssignment)
        .await?
        .into_iter()
        .filter_map(|record| match record {
            Record::RoleAssignment(row) => Some(row),
            _ => None,
        })
        .collect();
    snapshot.finish().await?;
    Ok(rows)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; Result propagates fixture errors."
)]
async fn many_roles_add_repeat_and_release_exactly_the_named_holder() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    for role in ["advisor", "reviewer", "operator"] {
        let (project, workstream) = fixture.project().await?;
        let subject = if role == "advisor" {
            json!({"kind":"project","subject":project})
        } else {
            json!({"kind":"workstream","subject":workstream})
        };
        let base = u64::from(role == "advisor");
        for (offset, holder) in ["first", "second"].into_iter().enumerate() {
            accepted(
                &fixture
                    .api
                    .call(
                        "role_grant",
                        grant(
                            &fixture,
                            &project,
                            subject.clone(),
                            role,
                            person(holder),
                            base + u64::try_from(offset)?,
                        ),
                    )
                    .await?,
            );
        }
        let before = assignments(&fixture, &project).await?;
        let repeated = fixture
            .api
            .call(
                "role_grant",
                grant(
                    &fixture,
                    &project,
                    subject.clone(),
                    role,
                    person("first"),
                    base + 2,
                ),
            )
            .await?;
        accepted(&repeated);
        assert_eq!(
            repeated.pointer("/result/result/decision/change"),
            Some(&json!("alreadySo")),
            "{repeated}"
        );
        assert_eq!(assignments(&fixture, &project).await?, before);
        let missing = fixture.api.call("role_release", fixture.request(&project, json!({"kind":"roleRelease","subject":subject,"role":role,"expectedRevision":base+2}))).await?;
        assert_eq!(refusal_kind(&missing), Some("invalidShape"), "{missing}");
        assert_eq!(
            missing.pointer("/result/result/decision/reasons/0/reason/field"),
            Some(&json!("holder"))
        );
        let first = before
            .iter()
            .find(|row| {
                row.holder
                    .declared_identity()
                    .is_some_and(|actor| serde_json::to_value(actor).ok() == Some(person("first")))
            })
            .ok_or("first assignment")?;
        let released = fixture.api.call("role_release", fixture.request(&project, json!({"kind":"roleRelease","subject":subject,"role":role,"holder":first.holder,"expectedRevision":base+2}))).await?;
        accepted(&released);
        let after = assignments(&fixture, &project).await?;
        let holders: Vec<_> = after
            .iter()
            .filter(|row| {
                row.subject() == first.subject()
                    && row.role() == first.role()
                    && row.ended.is_none()
            })
            .map(|row| serde_json::to_value(row.holder.declared_identity()))
            .collect::<Result<_, _>>()?;
        assert_eq!(holders, vec![person("second")]);
        assert!(
            after
                .iter()
                .any(|row| row.assignment_id == first.assignment_id
                    && matches!(row.ended, Some(RoleEnd::Released {})))
        );
    }
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; Result propagates fixture errors."
)]
async fn workstream_grants_check_authority_before_distinctness_in_both_directions()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let subject = json!({"kind":"workstream","subject":workstream});
    accepted(
        &fixture
            .api
            .call(
                "role_grant",
                grant(
                    &fixture,
                    &project,
                    subject.clone(),
                    "implementer",
                    person("builder"),
                    0,
                ),
            )
            .await?,
    );
    accepted(
        &fixture
            .api
            .call(
                "role_grant",
                grant(
                    &fixture,
                    &project,
                    subject.clone(),
                    "verifier",
                    person("checker"),
                    1,
                ),
            )
            .await?,
    );
    for role in ["implementer", "verifier", "reviewer", "advisor"] {
        let target = if role == "advisor" {
            json!({"kind":"project","subject":project})
        } else {
            subject.clone()
        };
        let revision = if role == "advisor" { 1 } else { 2 };
        let mut request = grant(
            &fixture,
            &project,
            target,
            role,
            person("builder"),
            revision,
        );
        request["actor"] = person("outsider");
        let refused = fixture.api.call("role_grant", request).await?;
        assert_eq!(refusal_kind(&refused), Some("roleRequired"), "{refused}");
        assert_eq!(
            refused.pointer("/result/result/decision/reasons/0/reason/requirement"),
            Some(&json!({"kind":"holds","role":"lead"})),
            "{refused}"
        );
    }
    for holder in ["builder", "checker"] {
        let refused = fixture
            .api
            .call(
                "role_grant",
                grant(
                    &fixture,
                    &project,
                    subject.clone(),
                    "reviewer",
                    person(holder),
                    2,
                ),
            )
            .await?;
        assert_eq!(refusal_kind(&refused), Some("pairNotDistinct"), "{refused}");
        assert_eq!(
            refused.pointer("/result/result/decision/reasons/0/reason/holder"),
            Some(&person(holder))
        );
    }
    accepted(
        &fixture
            .api
            .call(
                "role_grant",
                grant(
                    &fixture,
                    &project,
                    subject.clone(),
                    "reviewer",
                    person("reviewer"),
                    2,
                ),
            )
            .await?,
    );
    let before = assignments(&fixture, &project).await?;
    for role in ["implementer", "verifier"] {
        let refused = fixture
            .api
            .call(
                "role_grant",
                grant(
                    &fixture,
                    &project,
                    subject.clone(),
                    role,
                    person("reviewer"),
                    3,
                ),
            )
            .await?;
        assert_eq!(refusal_kind(&refused), Some("pairNotDistinct"), "{refused}");
    }
    assert_eq!(assignments(&fixture, &project).await?, before);
    let mut operator = grant(
        &fixture,
        &project,
        subject.clone(),
        "operator",
        person("runner"),
        3,
    );
    operator["actor"] = person("builder");
    accepted(&fixture.api.call("role_grant", operator).await?);
    let mut denied = grant(&fixture, &project, subject, "operator", person("other"), 4);
    denied["actor"] = person("checker");
    let denied = fixture.api.call("role_grant", denied).await?;
    assert_eq!(refusal_kind(&denied), Some("roleRequired"), "{denied}");
    assert_eq!(
        denied.pointer("/result/result/decision/reasons/0/reason/requirement"),
        Some(&json!({"kind":"leadOrWorkstreamImplementer"}))
    );
    let other_stream = WorkstreamId::generate();
    accepted(&fixture.api.call("workstream_create", fixture.request(&project, json!({"kind":"workstreamCreate","workstreamId":other_stream,"name":"other","goal":"scope"}))).await?);
    let mut wrong_stream = grant(
        &fixture,
        &project,
        json!({"kind":"workstream","subject":other_stream}),
        "operator",
        person("other"),
        0,
    );
    wrong_stream["actor"] = person("builder");
    assert_eq!(
        refusal_kind(&fixture.api.call("role_grant", wrong_stream).await?),
        Some("roleRequired")
    );
    let subject = json!({"kind":"workstream","subject":workstream});
    accepted(
        &fixture
            .api
            .call(
                "role_grant",
                grant(
                    &fixture,
                    &project,
                    subject.clone(),
                    "implementer",
                    person("next-builder"),
                    4,
                ),
            )
            .await?,
    );
    accepted(
        &fixture
            .api
            .call(
                "role_grant",
                grant(
                    &fixture,
                    &project,
                    subject,
                    "verifier",
                    person("next-checker"),
                    5,
                ),
            )
            .await?,
    );
    let moved = assignments(&fixture, &project).await?;
    for holder in ["builder", "checker"] {
        assert!(moved.iter().any(
            |row| serde_json::to_value(row.holder.declared_identity()).ok() == Some(person(holder))
                && matches!(row.ended, Some(RoleEnd::Replaced {}))
        ));
    }
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; Result propagates fixture errors."
)]
async fn active_lead_cannot_be_moved_through_role_grant() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let subject = json!({"kind":"project","subject":project});
    let unchanged = fixture
        .api
        .call(
            "role_grant",
            grant(
                &fixture,
                &project,
                subject.clone(),
                "lead",
                fixture.actor.clone(),
                1,
            ),
        )
        .await?;
    accepted(&unchanged);
    assert_eq!(
        unchanged.pointer("/result/result/decision/change"),
        Some(&json!("alreadySo"))
    );
    let mut outsider = grant(
        &fixture,
        &project,
        subject.clone(),
        "lead",
        fixture.actor.clone(),
        1,
    );
    outsider["actor"] = person("outsider");
    let outsider = fixture.api.call("role_grant", outsider).await?;
    assert_eq!(refusal_kind(&outsider), Some("roleRequired"), "{outsider}");
    let denied = fixture
        .api
        .call(
            "role_grant",
            grant(&fixture, &project, subject, "lead", person("new-lead"), 1),
        )
        .await?;
    assert_eq!(refusal_kind(&denied), Some("invalidShape"), "{denied}");
    assert_eq!(
        denied.pointer("/result/result/decision/reasons/0/reason/field"),
        Some(&json!("role"))
    );
    fixture.stop().await
}
