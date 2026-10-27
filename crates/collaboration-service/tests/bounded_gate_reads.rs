//! Read-mark and session-context gates read their rule operands beside unrelated retained history.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/read_cost_counter.rs"]
mod read_cost_counter;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use read_cost_counter::{ReadCost, current_cost};
use serde_json::json;
use sqlx::{ConnectOptions, Connection};
use std::collections::BTreeMap;

async fn unrelated_operands(
    store: &collaboration_runtime::NativeProjectStore,
    project: &ProjectId,
    size: usize,
) -> Result<(RecordRef, RecordRef), TestError> {
    let mut read = store.snapshot().await?;
    let template = read
        .record(&project.clone().into())
        .await?
        .ok_or("project")?;
    read.finish().await?;
    let envelope = |id: RecordId| RecordEnvelope {
        id,
        request: None,
        ..template.envelope().clone()
    };
    let other = Identity::Human {
        human_id: "unrelated holder".to_owned().try_into()?,
    };
    let RecordPlacement::Project(entry) = &template.envelope().position else {
        return Err("fixture project origin".into());
    };
    let mut write = store.begin_admission(entry.origin.clone()).await?;
    let mut first = None;
    for number in 0..size {
        let workstream = WorkstreamId::generate();
        write
            .append_record(Record::Workstream(Workstream {
                envelope: envelope(workstream.clone().into()),
                workstream_id: workstream.clone(),
                project_id: project.clone(),
                name: format!("unrelated {number}").try_into()?,
                goal: "unrelated authority".to_owned(),
                state: WorkstreamState::Open {},
                revision: SubjectRevision::INITIAL,
            }))
            .await?;
        let assignment = RoleAssignmentId::generate();
        let role = Record::RoleAssignment(RoleAssignment::new(RoleAssignmentProps {
            envelope: envelope(assignment.clone().into()),
            assignment_id: assignment,
            subject: RoleSubject::Workstream(workstream),
            role: AuthorityRole::Implementer,
            holder: RecordedActor::declared(other.clone()),
            granted_by: template.envelope().actor.clone(),
            model: None,
            lineage: None,
            ended: None,
            owner_grant: None,
        })?);
        let fact_id = FactId::generate();
        let fact = Record::Fact(Fact {
            envelope: envelope(fact_id.clone().into()),
            fact_id,
            kind: FactKind::EvaluatorResult {
                guard: format!("unused {number}"),
                subject: project.clone().into(),
                outcome: GuardOutcome::Met,
                evidence: Vec::new(),
                evaluator: EvaluatorRef {
                    actor: template.envelope().actor.clone(),
                    engine: None,
                    calibration: None,
                },
                executor: None,
            },
            links: vec![project.clone().into()],
            producer_time: template
                .envelope()
                .time
                .as_router_time()
                .ok_or("fixture router time")?,
            anchor: None,
            late: false,
        });
        first.get_or_insert_with(|| (role.reference(), fact.reference()));
        write.append_record(role).await?;
        write.append_record(fact).await?;
    }
    write.commit().await?;
    first.ok_or_else(|| "empty operand fixture".into())
}

#[allow(clippy::panic_in_result_fn)]
async fn gate_costs(size: usize) -> Result<BTreeMap<&'static str, ReadCost>, TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("store")?;
    let unrelated = unrelated_operands(&store, &project, size).await?;
    let reader: Identity = serde_json::from_value(fixture.actor.clone())?;
    let read = store.snapshot().await?;
    let covered = read.high_water();
    read.finish().await?;
    let before = current_cost();
    let mark = fixture
        ._admission
        .mark_covered_read(
            &project,
            &reader,
            ReadScope::Project {
                project_id: project.clone(),
            },
            covered,
        )
        .await?;
    let mark_cost = current_cost().since(before);
    assert_eq!(mark, ReadMarkOutcome::Advanced { through: covered });
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    let changed =
        sqlx::query("UPDATE role_assignments SET role = 'notARole' WHERE assignment_id = ?")
            .bind(unrelated.0.id.as_str())
            .execute(&mut connection)
            .await?;
    assert_eq!(changed.rows_affected(), 1);
    let changed = sqlx::query("UPDATE facts SET fact = json_set(fact, '$.fact.evaluator.actor.humanId', '') WHERE fact_id = ?")
        .bind(unrelated.1.id.as_str()).execute(&mut connection).await?;
    assert_eq!(changed.rows_affected(), 1);
    connection.close().await?;
    let mark = fixture
        ._admission
        .mark_covered_read(
            &project,
            &reader,
            ReadScope::Project {
                project_id: project.clone(),
            },
            covered,
        )
        .await?;
    assert_eq!(mark, ReadMarkOutcome::Unchanged {});
    let mut read = store.snapshot().await?;
    assert!(
        read.record(&unrelated.0).await.is_err(),
        "direct corrupt role must fail"
    );
    assert!(
        read.record(&unrelated.1).await.is_err(),
        "direct corrupt fact must fail"
    );
    read.finish().await?;
    fixture.stop().await?;
    Ok(BTreeMap::from([("covered read", mark_cost)]))
}

#[allow(clippy::panic_in_result_fn)]
async fn context_cost(
    size: usize,
) -> Result<(ReadCost, read_cost_counter::AlarmRunCosts), TestError> {
    let fixture = BoardFixture::start_context().await?;
    let (project, workstream) = fixture.project().await?;
    let session: SessionRef = serde_json::from_value(
        json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"context-reader"}),
    )?;
    accepted(&fixture.api.call("role_grant",fixture.request(&project,json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},"role":"implementer","holder":{"kind":"session","session":session},"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}))).await?);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("store")?;
    unrelated_operands(&store, &project, size).await?;
    let mut read = store.snapshot().await?;
    let template = read
        .record(&project.clone().into())
        .await?
        .ok_or("project")?;
    read.finish().await?;
    let mut own_facts = Vec::new();
    let RecordPlacement::Project(entry) = &template.envelope().position else {
        return Err("fixture project origin".into());
    };
    let mut write = store.begin_admission(entry.origin.clone()).await?;
    for number in 0..105 + size {
        let fact_id = FactId::generate();
        let mut subject_session = session.clone();
        if number < 105 {
            own_facts.push(fact_id.clone());
        } else {
            subject_session.session_id = format!("unrelated {number}").try_into()?;
        }
        write
            .append_record(Record::Fact(Fact {
                envelope: RecordEnvelope {
                    id: fact_id.clone().into(),
                    request: None,
                    ..template.envelope().clone()
                },
                fact_id,
                kind: FactKind::ScopeAcknowledgement {
                    session: subject_session,
                    subject: TrailSubject::Project,
                    paths: vec!["src".to_owned()],
                    reason: "scope proof".to_owned(),
                },
                links: vec![project.clone().into()],
                producer_time: template
                    .envelope()
                    .time
                    .as_router_time()
                    .ok_or("fixture router time")?,
                anchor: None,
                late: false,
            }))
            .await?;
    }
    write.commit().await?;
    let mut unrelated_workspace = None;
    for number in 0..size {
        let workspace = WorkspaceId::generate();
        unrelated_workspace.get_or_insert_with(|| workspace.clone());
        let mut create = fixture.header();
        create.as_object_mut().ok_or("header")?.extend(
            json!({"workspaceId":workspace,"name":format!("other workspace {number}"),"description":""})
                .as_object().ok_or("fields")?.clone(),
        );
        accepted(&fixture.api.call("workspace_create", create).await?);
    }
    // The pinned turso 0.8.1 facade exposes no public runtime row/VM counters.
    // Inspect the exact production query on its real native file; statement and
    // decode counts alone cannot detect a scan through unrelated JSON facts.
    let mut plan_connection = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .read_only(true)
        .connect()
        .await?;
    let statements_before = read_cost_counter::observed_statements().len();
    let alarms = read_cost_counter::observe_alarm_costs();
    let before = current_cost();
    let result = fixture
        .api
        .call("view_session_context", json!({"session":session}))
        .await?;
    let mut cost = current_cost().since(before);
    let alarms = alarms.finish().await?;
    eprintln!(
        "context SQL at size {size}: total={}, alarms={alarms:?}",
        cost.statements
    );
    cost.statements -= alarms.overlapping_statements;
    let observed = read_cost_counter::observed_statements();
    let queries = observed
        .iter()
        .skip(statements_before)
        .filter(|sql| sql.contains("FROM facts") && sql.contains("scopeAcknowledgement"))
        .collect::<Vec<_>>();
    assert_eq!(
        queries.len(),
        2,
        "the actual public request must page its 105 scope facts: {queries:?}"
    );
    for selected in queries {
        let query = format!("EXPLAIN QUERY PLAN {selected}");
        // Only SQL captured from this binary's real query event is composed;
        // session values stay bound and cannot become SQL syntax.
        let plan = sqlx::query(sqlx::AssertSqlSafe(query))
            .bind(session.endpoint.service_id.as_str())
            .bind(session.endpoint.endpoint_id.as_str())
            .bind(session.session_id.as_str())
            .bind("")
            .bind(100_i64)
            .fetch_all(&mut plan_connection)
            .await?;
        let details = plan
            .iter()
            .map(|row| sqlx::Row::try_get::<String, _>(row, "detail"))
            .collect::<Result<Vec<_>, _>>()?;
        eprintln!("native context query plan with {size} unrelated history: {details:?}");
        assert!(
            details
                .iter()
                .any(|detail| detail.contains("SEARCH") && detail.contains("context_scope_facts")),
            "context scope did not seek its session index: {details:?}"
        );
        assert!(
            !details.iter().any(|detail| detail.contains("SCAN facts")),
            "context scanned unrelated facts: {details:?}"
        );
    }
    plan_connection.close().await?;
    eprintln!("context fixture {size}: {cost:?}");
    assert_eq!(
        result.pointer("/result/kind"),
        Some(&json!("context")),
        "{result}"
    );
    assert_eq!(
        result
            .pointer("/result/payload/assignments")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    own_facts.sort();
    assert_eq!(
        result.pointer("/result/payload/acknowledgedScopes"),
        Some(&json!(own_facts)),
        "scope facts must cross the 100-key page without omissions or foreign sessions"
    );
    let unrelated_workspace = unrelated_workspace.ok_or("unrelated workspace")?;
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(fixture._directory.path().join("fleet-cache.sqlite"))
        .connect()
        .await?;
    let changed = sqlx::query("UPDATE cached_fleet_records SET payload = json_set(payload, '$.record.name', '') WHERE kind = 'workspace' AND id = ?")
        .bind(unrelated_workspace.as_str()).execute(&mut connection).await?;
    assert_eq!(changed.rows_affected(), 1);
    connection.close().await?;
    let result = fixture
        .api
        .call("view_session_context", json!({"session":session}))
        .await?;
    assert_eq!(
        result.pointer("/result/kind"),
        Some(&json!("context")),
        "unrelated workspace poisoned context: {result}"
    );
    let direct = fixture
        .api
        .call("workspace_show", json!({"workspaceId":unrelated_workspace}))
        .await?;
    assert_eq!(
        direct.pointer("/error/data/kind"),
        Some(&json!("invalidRecord")),
        "{direct}"
    );
    fixture.stop().await?;
    Ok((cost, alarms))
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn covered_reads_and_session_context_have_operand_bounded_cost() -> Result<(), TestError> {
    read_cost_counter::install();
    let small = gate_costs(8).await?;
    let large = gate_costs(80).await?;
    eprintln!("gate read costs: small={small:?}; large={large:?}");
    assert_eq!(small, large, "gates decoded unrelated roles or facts");
    let small = context_cost(8).await?;
    let large = context_cost(80).await?;
    assert_eq!(
        small.0, large.0,
        "context decoded unrelated fleet, role or scope-fact bodies"
    );
    assert_eq!(
        small.0.statements, 12,
        "exact foreground context SQL budget"
    );
    assert_eq!(
        large.0.statements, 12,
        "exact foreground context SQL budget"
    );
    // A read may overlap no alarm at all. When a fresh Fleet retention run
    // overlaps, it reads state and its expired prefix: exactly two statements.
    // bounded_fleet_writes also exercises this component at both fixture sizes.
    for alarm_costs in [&small.1, &large.1] {
        for (object_kind, runs) in &alarm_costs.by_object {
            assert_eq!(
                object_kind, "fleet",
                "unexpected context alarm: {alarm_costs:?}"
            );
            for run_statements in runs {
                assert_eq!(
                    *run_statements, 2,
                    "fresh retention run grew with unrelated history: {alarm_costs:?}"
                );
            }
        }
    }
    Ok(())
}
