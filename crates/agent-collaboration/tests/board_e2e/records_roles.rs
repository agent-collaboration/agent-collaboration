//! BR10 / VD §5a: literal authority, cardinality and history scenarios.
use super::{
    fixture::{BoardFixture, at, human},
    role_checks::{Roles, active, records, role_required},
};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};

pub async fn authority_and_cardinality(fixture: &mut BoardFixture) -> FixtureResult {
    // Wrong builds: non-lead grants, single-holder aliasing, missing history or empty effects.
    fixture.create().await?;
    let mut roles = Roles::new();
    reviewer_authority(fixture, &mut roles).await?;
    advisor_and_operator_authority(fixture, &mut roles).await?;
    lead_and_release_history(fixture, &mut roles).await?;
    owner_grants_and_implicit_subject(fixture, &mut roles).await?;
    super::records_reopen::assert_reopen(fixture).await
}

async fn reviewer_authority(fixture: &BoardFixture, roles: &mut Roles) -> FixtureResult {
    let verifier = human("verifier");
    let reviewer_a = human("reviewer-a");
    let reviewer_b = human("reviewer-b");
    let outsider = human("outsider");
    for (role, holder) in [("implementer", &fixture.helper), ("verifier", &verifier)] {
        roles
            .grant(
                fixture,
                &fixture.owner,
                "workstream",
                role,
                holder,
                json!("changed"),
            )
            .await?;
    }
    let denied = |reason: Value| json!({"kind":"rejected","reasons":[reason]});
    roles
        .grant(
            fixture,
            &outsider,
            "workstream",
            "reviewer",
            &fixture.helper,
            denied(role_required(json!({"kind":"holds","role":"lead"}))),
        )
        .await?;
    for holder in [&fixture.helper, &verifier] {
        roles
            .grant(
                fixture,
                &fixture.owner,
                "workstream",
                "reviewer",
                holder,
                denied(json!({"kind":"role","reason":{"kind":"pairNotDistinct","holder":holder}})),
            )
            .await?;
    }
    for holder in [&reviewer_a, &reviewer_b] {
        roles
            .grant(
                fixture,
                &fixture.owner,
                "workstream",
                "reviewer",
                holder,
                json!("changed"),
            )
            .await?;
    }
    assert_eq!(
        active(fixture, "reviewer").await?,
        vec![reviewer_a.clone(), reviewer_b.clone()]
    );
    roles
        .grant(
            fixture,
            &fixture.owner,
            "workstream",
            "reviewer",
            &reviewer_a,
            json!("alreadySo"),
        )
        .await?;
    roles
        .grant(
            fixture,
            &fixture.owner,
            "workstream",
            "implementer",
            &reviewer_a,
            denied(json!({"kind":"role","reason":{"kind":"pairNotDistinct","holder":reviewer_a}})),
        )
        .await?;
    Ok(())
}

async fn advisor_and_operator_authority(
    fixture: &BoardFixture,
    roles: &mut Roles,
) -> FixtureResult {
    let advisor = human("advisor");
    let operator_a = human("operator-a");
    let operator_b = human("operator-b");
    let outsider = human("outsider");
    let denied = |reason: Value| json!({"kind":"rejected","reasons":[reason]});
    roles
        .grant(
            fixture,
            &fixture.owner,
            "project",
            "advisor",
            &advisor,
            json!("changed"),
        )
        .await?;
    roles
        .grant(
            fixture,
            &fixture.helper,
            "workstream",
            "operator",
            &operator_a,
            json!("changed"),
        )
        .await?;
    roles
        .grant(
            fixture,
            &fixture.owner,
            "workstream",
            "operator",
            &operator_b,
            json!("changed"),
        )
        .await?;
    assert_eq!(
        active(fixture, "operator").await?,
        vec![operator_a, operator_b]
    );
    roles
        .grant(
            fixture,
            &advisor,
            "workstream",
            "operator",
            &outsider,
            denied(role_required(json!({"kind":"leadOrWorkstreamImplementer"}))),
        )
        .await?;
    Ok(())
}

async fn lead_and_release_history(fixture: &BoardFixture, roles: &mut Roles) -> FixtureResult {
    let outsider = human("outsider");
    let reviewer_a = human("reviewer-a");
    let reviewer_b = human("reviewer-b");
    let denied = |reason: Value| json!({"kind":"rejected","reasons":[reason]});
    roles
        .grant(
            fixture,
            &fixture.owner,
            "project",
            "lead",
            &fixture.owner,
            json!("alreadySo"),
        )
        .await?;
    roles
        .grant(
            fixture,
            &outsider,
            "project",
            "lead",
            &fixture.owner,
            denied(role_required(json!({"kind":"holds","role":"lead"}))),
        )
        .await?;
    roles
        .grant(
            fixture,
            &fixture.owner,
            "project",
            "lead",
            &outsider,
            denied(json!({"kind":"invalidShape","reason":{"field":"role"}})),
        )
        .await?;
    roles
        .change(
            fixture,
            &fixture.owner,
            "workstream",
            json!({"kind":"roleRelease","role":"reviewer","holder":null}),
            denied(json!({"kind":"invalidShape","reason":{"field":"holder"}})),
        )
        .await?;
    roles
        .change(
            fixture,
            &fixture.owner,
            "workstream",
            json!({"kind":"roleRelease","role":"reviewer","holder":reviewer_a}),
            json!("changed"),
        )
        .await?;
    assert_eq!(active(fixture, "reviewer").await?, vec![reviewer_b]);
    let ended = records(fixture)
        .await?
        .into_iter()
        .filter(|row| {
            row.get("role") == Some(&json!("reviewer")) && row.get("ended") != Some(&Value::Null)
        })
        .map(|row| Ok(json!([at(&row, "/holder")?, at(&row, "/ended")?])))
        .collect::<FixtureResult<Vec<_>>>()?;
    assert_eq!(ended, vec![json!([reviewer_a,{"kind":"released"}])]);
    assert_eq!(
        active(fixture, "implementer").await?,
        vec![fixture.helper.clone()]
    );
    Ok(())
}

async fn owner_grants_and_implicit_subject(
    fixture: &BoardFixture,
    roles: &mut Roles,
) -> FixtureResult {
    for (scope, role) in [
        ("project", "advisor"),
        ("workstream", "reviewer"),
        ("workstream", "operator"),
    ] {
        roles
            .grant(
                fixture,
                &fixture.owner,
                scope,
                role,
                &fixture.owner,
                json!("changed"),
            )
            .await?;
    }
    let row = super::fixture::identity();
    fixture.accept("work_trail_add",fixture.request(&fixture.owner,json!({"kind":"workTrailAdd",
        "rowId":row,"subject":null,"text":"lead retains implicit project subject","tags":[],"links":[]}))).await?;
    let shown=fixture.host()?.mcp("work_trail_show",json!({"projectId":fixture.project,
        "reader":{"kind":"anonymous"},"selection":{"kind":"forward","after":{"epoch":1,"position":0}},
        "limit":100,"subject":null,"tags":[],"includeRedacted":false})).await?;
    let rows = at(&shown, "/value/page/rows")?
        .as_array()
        .ok_or("trail rows absent")?;
    assert_eq!(rows.len(), 1);
    let stored = rows.first().ok_or("trail row absent")?;
    assert_eq!(stored.get("rowId"), Some(&json!(row)));
    assert_eq!(stored.get("subject"), Some(&json!({"kind":"project"})));
    assert_eq!(
        stored.get("text"),
        Some(&json!("lead retains implicit project subject"))
    );
    Ok(())
}
