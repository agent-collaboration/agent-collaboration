//! Public role mutations and independent assignment/history readbacks shared by journeys.
use super::fixture::{BoardFixture, accepted, at, decision};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};

pub fn role_required(requirement: Value) -> Value {
    json!({"kind":"role","reason":{"kind":"roleRequired","requirement":requirement}})
}

pub fn rejected(reply: &Value, reasons: Value) -> FixtureResult {
    assert_eq!(
        decision(reply)?,
        &json!({"kind":"rejected","reasons":reasons})
    );
    Ok(())
}

pub async fn records(fixture: &BoardFixture) -> FixtureResult<Vec<Value>> {
    let page = fixture
        .host()?
        .mcp(
            "role_list",
            json!({"projectId":fixture.project,
        "page":{"limit":100,"cursor":null}}),
        )
        .await?;
    assert_eq!(page.get("nextCursor"), Some(&Value::Null));
    at(&page, "/records")?
        .as_array()
        .ok_or("role rows absent")?
        .iter()
        .map(|row| at(row, "/record").cloned())
        .collect()
}

pub async fn active(fixture: &BoardFixture, role: &str) -> FixtureResult<Vec<Value>> {
    let mut holders = records(fixture)
        .await?
        .into_iter()
        .filter(|row| {
            row.get("role") == Some(&json!(role)) && row.get("ended") == Some(&Value::Null)
        })
        .map(|row| at(&row, "/holder").cloned())
        .collect::<FixtureResult<Vec<_>>>()?;
    holders.sort_by_key(Value::to_string);
    Ok(holders)
}

pub struct Roles {
    project_revision: u64,
    stream_revision: u64,
}
impl Roles {
    pub fn new() -> Self {
        Self {
            project_revision: 1,
            stream_revision: 0,
        }
    }

    pub async fn change(
        &mut self,
        fixture: &BoardFixture,
        actor: &Value,
        scope: &str,
        operation: Value,
        expected: Value,
    ) -> FixtureResult<Value> {
        let revision = if scope == "project" {
            &mut self.project_revision
        } else {
            &mut self.stream_revision
        };
        let subject = if scope == "project" {
            &fixture.project
        } else {
            &fixture.workstream
        };
        let operation = super::fixture::fields(
            json!({"subject":{"kind":scope,"subject":subject},
            "expectedRevision":revision}),
            operation,
        )?;
        let verb = if operation.get("kind") == Some(&json!("roleGrant")) {
            "grant"
        } else {
            "release"
        };
        let before = records(fixture).await?;
        let reply = fixture
            .paired_write(
                &["role", verb],
                &format!("role_{verb}"),
                fixture.request(actor, operation.clone()),
            )
            .await?;
        if expected.get("kind") == Some(&json!("rejected")) {
            assert_eq!(decision(&reply)?, &expected);
        } else {
            assert_eq!(accepted(&reply)?.get("change"), Some(&expected));
        }
        let after = records(fixture).await?;
        if expected == json!("changed") {
            *revision += 1;
            let changed = assert_role_transition(&operation, &before, &after)?;
            assert_role_effects(fixture, &reply, &after, &changed).await?;
        } else {
            assert_eq!(
                before, after,
                "refusal/AlreadySo must not alter assignment history"
            );
        }
        Ok(reply)
    }

    pub async fn grant(
        &mut self,
        fixture: &BoardFixture,
        actor: &Value,
        scope: &str,
        role: &str,
        holder: &Value,
        expected: Value,
    ) -> FixtureResult<Value> {
        self.change(
            fixture,
            actor,
            scope,
            json!({"kind":"roleGrant","role":role,"holder":holder,
            "model":null,"lineage":null,"ownerGrant":null}),
            expected,
        )
        .await
    }
}

fn same_assignment(row: &Value, operation: &Value) -> bool {
    row.get("subject") == operation.get("subject") && row.get("role") == operation.get("role")
}

fn assignment_by_id<'a>(rows: &'a [Value], assignment: &Value) -> FixtureResult<&'a Value> {
    rows.iter()
        .find(|row| row.get("assignmentId") == Some(assignment))
        .ok_or_else(|| {
            format!("assignment {assignment} missing from persisted role history").into()
        })
}

fn assert_role_transition(
    operation: &Value,
    before: &[Value],
    after: &[Value],
) -> FixtureResult<Vec<Value>> {
    let granting = operation.get("kind") == Some(&json!("roleGrant"));
    let holder = at(operation, "/holder")?;
    let mut changed = Vec::new();
    if granting {
        assert_eq!(
            after.len(),
            before.len() + 1,
            "changed grant must persist a new assignment"
        );
        let added = after
            .iter()
            .filter(|row| {
                same_assignment(row, operation)
                    && row.get("holder") == Some(holder)
                    && row.get("ended") == Some(&Value::Null)
                    && !before
                        .iter()
                        .any(|old| old.get("assignmentId") == row.get("assignmentId"))
            })
            .collect::<Vec<_>>();
        assert_eq!(
            added.len(),
            1,
            "requested subject/role/holder must be independently readable"
        );
        let added = added.first().ok_or("new assignment absent")?;
        assert_eq!(added.get("model"), operation.get("model"));
        assert_eq!(added.get("lineage"), operation.get("lineage"));
        changed.push(at(added, "/assignmentId")?.clone());
    } else {
        assert_eq!(
            after.len(),
            before.len(),
            "release must retain assignment history"
        );
    }
    let single_holder = matches!(
        operation.get("role").and_then(Value::as_str),
        Some("lead" | "implementer" | "verifier")
    );
    for previous in before {
        let assignment = at(previous, "/assignmentId")?;
        let current = assignment_by_id(after, assignment)?;
        let ending = same_assignment(previous, operation)
            && previous.get("ended") == Some(&Value::Null)
            && ((!granting && previous.get("holder") == Some(holder))
                || (granting && single_holder));
        if ending {
            let expected = if granting { "replaced" } else { "released" };
            assert_eq!(
                current.get("ended"),
                Some(&json!({"kind":expected})),
                "assignment history must close at the requested transition"
            );
            changed.push(assignment.clone());
        } else {
            assert_eq!(
                current, previous,
                "unrelated or already ended assignment changed"
            );
        }
    }
    assert!(
        !changed.is_empty(),
        "changed role transition has no persisted assignment effect"
    );
    Ok(changed)
}

async fn assert_role_effects(
    fixture: &BoardFixture,
    reply: &Value,
    after: &[Value],
    changed: &[Value],
) -> FixtureResult {
    let entries = at(accepted(reply)?, "/effects/entries")?
        .as_array()
        .ok_or("effect entries absent")?;
    let references = entries
        .iter()
        .map(|entry| {
            entry
                .as_array()
                .and_then(|entry| entry.first())
                .ok_or_else(|| "effect reference absent".into())
        })
        .collect::<FixtureResult<Vec<_>>>()?;
    let references = references
        .into_iter()
        .filter(|reference| reference.get("kind") == Some(&json!("roleAssignment")))
        .collect::<Vec<_>>();
    let mut actual = references
        .iter()
        .map(|reference| at(reference, "/id").cloned())
        .collect::<FixtureResult<Vec<_>>>()?;
    let mut expected = changed.to_vec();
    actual.sort_by_key(Value::to_string);
    expected.sort_by_key(Value::to_string);
    assert_eq!(
        actual, expected,
        "response effects must name every independently observed role transition"
    );
    for reference in references {
        let assignment = at(reference, "/id")?;
        let cli = fixture
            .cli(&[
                "role",
                "show",
                "--project-id",
                &fixture.project,
                "--id",
                assignment.as_str().ok_or("assignment ID absent")?,
            ])
            .await?;
        let mcp = fixture
            .host()?
            .mcp(
                "role_show",
                json!({"projectId":fixture.project,"record":reference}),
            )
            .await?;
        assert_eq!(at(&cli, "/record")?, &mcp);
        assert_eq!(at(&mcp, "/record")?, assignment_by_id(after, assignment)?);
    }
    Ok(())
}
