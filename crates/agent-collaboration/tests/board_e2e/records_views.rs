//! BR07 / VD §5a U1,U2,U4-group,U79: applicable pending work, dependencies and read-only summaries.
use super::fixture::machine_progress;
use super::{
    fixture::{BoardFixture, accepted, at, human, identity},
    record_inputs::{evidence, reference, task, trail},
    role_checks::Roles,
};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};

async fn read(fixture: &BoardFixture, tool: &str, request: Value) -> FixtureResult<Value> {
    fixture.host()?.mcp(tool, request).await
}

fn ids(value: &Value, pointer: &str, field: &str) -> FixtureResult<Vec<Value>> {
    at(value, pointer)?
        .as_array()
        .ok_or("record array absent")?
        .iter()
        .map(|row| at(row, field).cloned())
        .collect()
}

async fn await_cut(fixture: &BoardFixture) -> FixtureResult {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let progress = machine_progress::observe(fixture.host()?, None).await?;
            let view = read(
                fixture,
                "view_work",
                json!({"projectId":fixture.project,"includeRemoved":false}),
            )
            .await?;
            if at(&view, "/asOf")? == at(&view, "/replicatedThrough")? {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            machine_progress::observe(fixture.host()?, Some(progress)).await?;
        }
    })
    .await?
}

struct PendingViewCase {
    dependent: String,
    prerequisite: String,
    foreign: String,
    subtask: String,
    reader: Value,
    plan: String,
    subject: Value,
    handback: String,
    other: String,
    question: String,
}
struct PendingTasks {
    dependent: String,
    prerequisite: String,
    foreign: String,
    subtask: String,
}

pub async fn pending_views(fixture: &mut BoardFixture) -> FixtureResult {
    // Preserve the state-dependent sequence: read-only summaries, settlement, redaction, sibling damage.
    fixture.create().await?;
    let case = prepare_case(fixture).await?;
    let before_marks = read_marks(fixture).await?;
    await_cut(fixture).await?;
    observe_card(fixture, &case).await?;
    observe_work(fixture, &case).await?;
    observe_owner_and_lead(fixture, &case).await?;
    assert_eq!(
        read_marks(fixture).await?,
        before_marks,
        "summaries must not acknowledge records"
    );
    remove_subtask(fixture, &case).await?;
    settle_and_redact(fixture, &case).await?;
    isolate_corrupt_sibling(fixture, &case).await?;
    super::records_reopen::assert_reopen(fixture).await
}

async fn prepare_tasks(fixture: &BoardFixture) -> FixtureResult<PendingTasks> {
    let dependent = task(fixture, "dependent").await?;
    let prerequisite = task(fixture, "prerequisite").await?;
    let foreign = task(fixture, "other subject").await?;
    fixture
        .accept(
            "task_link",
            fixture.request(
                &fixture.owner,
                json!({"kind":"taskLink","link":{
        "from":dependent,"kind":"blockedBy","to":{"project":fixture.project,"task":prerequisite}}}),
            ),
        )
        .await?;
    let subtask = identity();
    accepted(at(
        &fixture
            .cli(&[
                "subtask",
                "add",
                "--project-id",
                &fixture.project,
                "--subtask-id",
                &subtask,
                "--task-id",
                &dependent,
                "--text",
                "verified checklist item",
                "--place",
                "80",
                "--actor",
                &fixture.owner.to_string(),
            ])
            .await?,
        "/record",
    )?)?;
    fixture
        .accept(
            "subtask_check",
            fixture.request(
                &fixture.owner,
                json!({"kind":"subtaskCheck","subtaskId":subtask,"checked":true}),
            ),
        )
        .await?;
    let page = read(
        fixture,
        "task_list",
        json!({"projectId":fixture.project,"page":{"limit":1}}),
    )
    .await?;
    assert_eq!(
        ids(&page, "/records", "/record/taskId")?,
        vec![json!(dependent)]
    );
    assert!(page.get("nextCursor").is_some_and(Value::is_string));
    Ok(PendingTasks {
        dependent,
        prerequisite,
        foreign,
        subtask,
    })
}

async fn prepare_case(fixture: &BoardFixture) -> FixtureResult<PendingViewCase> {
    let reader = human("reader");
    let mut roles = Roles::new();
    for (role, holder) in [("implementer", &fixture.helper), ("verifier", &reader)] {
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
    let PendingTasks {
        dependent,
        prerequisite,
        foreign,
        subtask,
    } = prepare_tasks(fixture).await?;
    let plan = "11".repeat(32);
    fixture
        .accept(
            "project_publish_plan",
            fixture.request(
                &fixture.owner,
                json!({"kind":"projectPublishPlan",
        "hash":plan,"document":evidence("task",&dependent),"expectedRevision":0}),
            ),
        )
        .await?;
    let subject = json!({"kind":"task","subject":dependent});
    let handback = trail(
        fixture,
        &fixture.helper,
        subject.clone(),
        "handback",
        "implementation ready",
        vec![],
    )
    .await?;
    let other = trail(
        fixture,
        &fixture.helper,
        json!({"kind":"task","subject":foreign}),
        "handback",
        "other subject handback",
        vec![],
    )
    .await?;
    let question = trail(
        fixture,
        &fixture.owner,
        subject.clone(),
        "owner-question",
        "owner choice blocks task",
        vec![evidence("task", &dependent)],
    )
    .await?;
    read(
        fixture,
        "feed_read",
        json!({"projectId":fixture.project,"reader":{"kind":"identified","reader":reader},
        "after":{"epoch":1,"position":0},"limit":100,"includeRedacted":true}),
    )
    .await?;
    Ok(PendingViewCase {
        dependent,
        prerequisite,
        foreign,
        subtask,
        reader,
        plan,
        subject,
        handback,
        other,
        question,
    })
}

async fn read_marks(fixture: &mut BoardFixture) -> FixtureResult<Vec<String>> {
    fixture.stop_host().await?;
    let result = super::records_reopen::stopped_values(fixture,
        "SELECT json_array(reader_actor,scope_kind,scope_id,through_epoch,through_position) AS value FROM read_marks ORDER BY reader_actor,scope_kind,scope_id").await;
    let reopened = fixture.reopen_host().await;
    match (result, reopened) {
        (Ok(rows), Ok(())) => Ok(rows),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(read), Err(reopen)) => Err(format!("read marks: {read}; reopen: {reopen}").into()),
    }
}

fn card_request(fixture: &BoardFixture, case: &PendingViewCase) -> Value {
    json!({"projectId":fixture.project,"actor":fixture.helper,"subject":case.subject})
}

async fn observe_card(fixture: &BoardFixture, case: &PendingViewCase) -> FixtureResult {
    let PendingViewCase {
        plan,
        subject,
        handback,
        question,
        ..
    } = case;
    let card_request = card_request(fixture, case);
    let card = read(fixture, "view_card", card_request.clone()).await?;
    let cli = fixture
        .cli(&[
            "view",
            "card",
            "--project-id",
            &fixture.project,
            "--actor",
            &fixture.helper.to_string(),
            "--subject",
            &subject.to_string(),
        ])
        .await?;
    assert_eq!(at(&cli, "/record")?, &card);
    assert_eq!(
        json!([
            card.get("planRevision"),
            card.get("assignment"),
            card.get("pendingHandbacks"),
            card.get("pendingDecisions"),
            card.get("nextObligation")
        ]),
        json!([plan,{"kind":"implementer","subject":fixture.workstream},
        [reference("workTrailRow",handback)],[reference("workTrailRow",question)],null])
    );
    assert_eq!(
        ids(&card, "/recentTrail", "/rowId")?,
        vec![json!(question), json!(handback)]
    );
    assert_eq!(
        json!([
            card.pointer("/counts/task"),
            card.pointer("/counts/workTrailRow"),
            card.pointer("/counts/subtask")
        ]),
        json!([3, 3, 1])
    );
    Ok(())
}

async fn observe_work(fixture: &BoardFixture, case: &PendingViewCase) -> FixtureResult {
    let PendingViewCase {
        reader,
        prerequisite,
        dependent,
        foreign,
        ..
    } = case;
    let work = fixture
        .cli(&["view", "work", "--project-id", &fixture.project])
        .await?;
    let work = at(&work, "/record")?;
    let lane = at(work, "/workstreams")?
        .as_array()
        .and_then(|lanes| lanes.first())
        .ok_or("workstream absent")?;
    assert_eq!(
        json!([
            lane.get("id"),
            lane.get("implementer"),
            lane.get("verifier")
        ]),
        json!([fixture.workstream, fixture.helper, reader])
    );
    assert_eq!(
        ids(lane, "/tasks", "/id")?,
        vec![json!(prerequisite), json!(dependent), json!(foreign)]
    );
    assert_eq!(
        work.get("readyQueue"),
        Some(&json!([prerequisite, foreign]))
    );
    assert_eq!(
        work.get("blocked"),
        Some(&json!([{"task":dependent,"blockers":[prerequisite],"byDropped":false}]))
    );
    let checked = at(lane, "/tasks")?
        .as_array()
        .ok_or("tasks absent")?
        .iter()
        .find(|row| row.get("id") == Some(&json!(dependent)))
        .ok_or("dependent task absent")?;
    assert_eq!(
        json!([checked.get("subtasksChecked"), checked.get("subtasksTotal")]),
        json!([1, 1])
    );
    Ok(())
}

async fn observe_owner_and_lead(fixture: &BoardFixture, case: &PendingViewCase) -> FixtureResult {
    let PendingViewCase {
        subject,
        handback,
        other,
        question,
        dependent,
        ..
    } = case;
    let started = chrono::Utc::now();
    let mut owner = read(fixture, "view_owner", json!({})).await?;
    let cli = fixture.cli(&["view", "owner"]).await?;
    let mut cli = at(&cli, "/record")?.clone();
    let ended = chrono::Utc::now();
    for value in [&owner, &cli] {
        let stamp = chrono::DateTime::parse_from_rfc3339(
            at(value, "/asOf")?.as_str().ok_or("owner time")?,
        )?;
        assert_eq!(stamp.offset().local_minus_utc(), 0);
        assert!(started <= stamp && stamp <= ended);
    }
    owner.as_object_mut().ok_or("owner object")?.remove("asOf");
    cli.as_object_mut().ok_or("owner object")?.remove("asOf");
    assert_eq!(owner, cli);
    let waiting = at(&owner, "/waiting")?
        .as_array()
        .ok_or("owner waiting absent")?;
    assert_eq!(waiting.len(), 1);
    let waiting = waiting.first().ok_or("owner question absent")?;
    assert_eq!(
        json!([
            waiting.get("project"),
            waiting.get("subject"),
            waiting.get("blocks")
        ]),
        json!([
            fixture.project,
            reference("workTrailRow", question),
            [reference("task", dependent)]
        ])
    );
    let load = read(fixture, "view_lead_load", json!({"lead":fixture.owner})).await?;
    assert_eq!(
        ids(&load, "/owed", "/subject/id")?,
        vec![json!(handback), json!(other)]
    );
    let lead = read(
        fixture,
        "view_card",
        json!({"projectId":fixture.project,"actor":fixture.owner,"subject":subject}),
    )
    .await?;
    assert_eq!(
        lead.get("assignment"),
        Some(&json!({"kind":"lead","subject":fixture.project}))
    );
    assert_eq!(
        lead.pointer("/nextObligation/subject"),
        Some(&reference("workTrailRow", handback))
    );
    Ok(())
}

async fn remove_subtask(fixture: &BoardFixture, case: &PendingViewCase) -> FixtureResult {
    let subtask = &case.subtask;
    fixture
        .accept(
            "subtask_remove",
            fixture.request(
                &fixture.owner,
                json!({"kind":"subtaskRemove","subtaskId":subtask}),
            ),
        )
        .await?;
    let removed = read(
        fixture,
        "subtask_show",
        json!({"projectId":fixture.project,"record":reference("subtask",subtask)}),
    )
    .await?;
    assert_eq!(removed.pointer("/record/removed"), Some(&json!(true)));
    Ok(())
}

async fn set_redacted(fixture: &BoardFixture, row: &str, redacted: bool) -> FixtureResult {
    fixture
        .accept(
            "work_trail_redact",
            fixture.request(
                &fixture.owner,
                json!({"kind":"workTrailRedact","rowId":row,"redacted":redacted}),
            ),
        )
        .await?;
    Ok(())
}
fn decisions_request(fixture: &BoardFixture) -> Value {
    json!({"projectId":fixture.project,"reader":{"kind":"anonymous"},
        "selection":{"kind":"decisions","after":null,"before":null,"rangeTop":null},
        "limit":100,"subject":null,"tags":[],"includeRedacted":false})
}

async fn settle_and_redact(fixture: &BoardFixture, case: &PendingViewCase) -> FixtureResult {
    let PendingViewCase {
        subject,
        handback,
        question,
        ..
    } = case;
    let card_request = card_request(fixture, case);
    let settlement = trail(
        fixture,
        &fixture.owner,
        subject.clone(),
        "decision",
        "lead settles both rows",
        vec![
            evidence("workTrailRow", handback),
            evidence("workTrailRow", question),
        ],
    )
    .await?;
    let settled = read(fixture, "view_card", card_request.clone()).await?;
    assert_eq!(
        json!([
            settled.get("pendingHandbacks"),
            settled.get("pendingDecisions")
        ]),
        json!([[], []])
    );
    assert_eq!(
        read(fixture, "view_owner", json!({})).await?.get("waiting"),
        Some(&json!([]))
    );
    set_redacted(fixture, &settlement, true).await?;
    let revived = read(fixture, "view_card", card_request.clone()).await?;
    assert_eq!(
        json!([
            revived.get("pendingHandbacks"),
            revived.get("pendingDecisions")
        ]),
        json!([
            [reference("workTrailRow", handback)],
            [reference("workTrailRow", question)]
        ])
    );
    let register = decisions_request(fixture);
    assert_eq!(
        read(fixture, "work_trail_show", register.clone())
            .await?
            .pointer("/value/page/rows"),
        Some(&json!([]))
    );
    let feed = fixture.feed().await?;
    let contents = at(&feed, "/value/entries")?
        .as_array()
        .ok_or("entries absent")?
        .iter()
        .filter(|entry| entry.pointer("/record/id") == Some(&json!(settlement)))
        .map(|entry| at(entry, "/content/kind").cloned())
        .collect::<FixtureResult<Vec<_>>>()?;
    assert_eq!(contents, vec![json!("redacted")]);
    set_redacted(fixture, &settlement, false).await?;
    assert_eq!(
        ids(
            &read(fixture, "work_trail_show", register).await?,
            "/value/page/rows",
            "/rowId"
        )?,
        vec![json!(settlement)]
    );
    Ok(())
}

async fn isolate_corrupt_sibling(
    fixture: &mut BoardFixture,
    case: &PendingViewCase,
) -> FixtureResult {
    let PendingViewCase {
        subject,
        other,
        foreign,
        ..
    } = case;
    let card_request = card_request(fixture, case);
    let next = trail(
        fixture,
        &fixture.helper,
        subject.clone(),
        "handback",
        "second owed handback",
        vec![],
    )
    .await?;
    await_cut(fixture).await?;
    let healthy = read(fixture, "view_card", card_request.clone()).await?;
    assert_eq!(
        healthy.get("pendingHandbacks"),
        Some(&json!([reference("workTrailRow", &next)]))
    );
    fixture.stop_host().await?;
    super::records_reopen::damage_trail_link(fixture, other).await?;
    fixture.reopen_host().await?;
    assert_eq!(
        read(fixture, "view_card", card_request.clone()).await?,
        healthy
    );
    let corrupt = fixture
        .host()?
        .mcp_reply(
            "view_card",
            json!({"projectId":fixture.project,"actor":fixture.helper,
        "subject":{"kind":"task","subject":foreign}}),
        )
        .await?;
    assert!(corrupt.is_error);
    Ok(())
}
