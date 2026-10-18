//! Read-mark and move gates read their rule operands beside unrelated retained history.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/read_cost_counter.rs"]
mod read_cost_counter;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use read_cost_counter::{ReadCost, current_cost};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{ConnectOptions, Connection};
use std::collections::BTreeMap;

async fn prepare_move(fixture: &BoardFixture, project: &ProjectId) -> Result<(), TestError> {
    let source = json!({"initialStatus":"draft","edges":[{"name":"finish","from":"draft","to":"done","requiredRole":"lead","guards":[{"name":"proof","evaluator":fixture.actor}]}]}).to_string();
    let mut register = fixture.header();
    register.as_object_mut().ok_or("header")?.extend(
        json!({"workspaceId":fixture.workspace,"name":"gate flow","version":"v1","source":source})
            .as_object()
            .ok_or("fields")?
            .clone(),
    );
    accepted(&fixture.api.call("process_register", register).await?);
    let definition = json!({"name":"gate flow","version":"v1","sourceHash":format!("{:x}",Sha256::digest(source.as_bytes()))});
    accepted(&fixture.api.call("project_pin_definition",fixture.request(project,json!({"kind":"projectPinDefinition","definition":definition,"expectedRevision":0}))).await?);
    let evaluator = json!({"actor":{"kind":"declared","identity":fixture.actor},"installation":"00000000-0000-4000-8000-000000000001","machine":"gate proof"});
    accepted(&fixture.api.call("fact_record",fixture.request(project,json!({"kind":"factRecord","fact":{"factId":FactId::generate(),"kind":{"kind":"evaluatorResult","fact":{"guard":"proof","subject":{"kind":"project","id":project},"outcome":"met","evidence":[],"evaluator":{"actor":evaluator,"engine":null,"calibration":null},"executor":null}},"links":[{"kind":"project","id":project}],"producerTime":"2026-10-12T00:00:00Z","anchor":null}}))).await?);
    Ok(())
}

async fn unrelated_operands(
    store: &message_board_storage::ProjectStore,
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
    let mut write = store.begin_admission().await?;
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
            holder: RecordedActor {
                actor: ActorKind::Declared(other.clone()),
                ..template.envelope().actor.clone()
            },
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
            producer_time: template.envelope().router_time,
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
    prepare_move(&fixture, &project).await?;
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
    let check =
        json!({"projectId":project,"actor":reader,"moveName":"finish","expectedRevision":1});
    let before = current_cost();
    let verdict = fixture.api.call("move_check", check.clone()).await?;
    let move_cost = current_cost().since(before);
    assert_eq!(
        verdict.pointer("/result/kind"),
        Some(&json!("allowed")),
        "{verdict}"
    );

    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    for (reference, path) in [
        (&unrelated.0, "$.record.holder.machine"),
        (&unrelated.1, "$.record.kind.fact.evaluator.actor.machine"),
    ] {
        let changed = sqlx::query("UPDATE record_identities SET payload = json_set(payload, ?, '') WHERE kind = ? AND id = ?")
            .bind(path).bind(serde_json::to_value(reference.kind)?.as_str().ok_or("kind")?)
            .bind(reference.id.as_str()).execute(&mut connection).await?;
        assert_eq!(changed.rows_affected(), 1);
    }
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
    let verdict = fixture.api.call("move_check", check).await?;
    assert_eq!(
        verdict.pointer("/result/kind"),
        Some(&json!("allowed")),
        "{verdict}"
    );
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
    Ok(BTreeMap::from([
        ("covered read", mark_cost),
        ("move check", move_cost),
    ]))
}

#[allow(clippy::panic_in_result_fn)]
async fn context_cost(size: usize) -> Result<ReadCost, TestError> {
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
    let mut write = store.begin_admission().await?;
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
                producer_time: template.envelope().router_time,
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
    let before = current_cost();
    let result = fixture
        .api
        .call("hook_context", json!({"session":session}))
        .await?;
    let cost = current_cost().since(before);
    eprintln!("context fixture {size}: {cost:?}");
    assert_eq!(
        result.pointer("/result/kind"),
        Some(&json!("ok")),
        "{result}"
    );
    assert_eq!(
        result
            .pointer("/result/payload/payload/assignments")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    own_facts.sort();
    assert_eq!(
        result.pointer("/result/payload/payload/acknowledgedScopes"),
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
        .call("hook_context", json!({"session":session}))
        .await?;
    assert_eq!(
        result.pointer("/result/kind"),
        Some(&json!("ok")),
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
    Ok(cost)
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn covered_read_and_move_checks_have_operand_bounded_cost() -> Result<(), TestError> {
    read_cost_counter::install();
    let small = gate_costs(8).await?;
    let large = gate_costs(80).await?;
    eprintln!("gate read costs: small={small:?}; large={large:?}");
    assert_eq!(small, large, "gates decoded unrelated roles or facts");
    let small = context_cost(8).await?;
    let large = context_cost(80).await?;
    assert_eq!(
        small, large,
        "context decoded unrelated fleet, role or scope-fact bodies"
    );
    Ok(())
}
