//! The public work view does not select or decode unrelated fact history.
// This fixture also supplies other board journeys.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "Shared test fixture helpers serve different callers."
)]
mod board_fixture;
use board_fixture::{BoardFixture, TestError};
use message_board::*;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tracing::field::{Field, Visit};
use tracing_subscriber::{Layer, layer::Context, prelude::*};

struct Statements(Arc<Mutex<Vec<String>>>);
#[derive(Default)]
struct SqlField(String);
impl Visit for SqlField {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "db.statement" {
            self.0 = value.to_owned();
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "db.statement" {
            self.0 = format!("{value:?}");
        }
    }
}
impl<S: tracing::Subscriber> Layer<S> for Statements {
    #[expect(
        clippy::unwrap_used,
        reason = "a poisoned SQL observer must fail the proof"
    )]
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        if event.metadata().target() == "sqlx::query" {
            let mut sql = SqlField::default();
            event.record(&mut sql);
            if !sql.0.is_empty() {
                self.0.lock().unwrap().push(sql.0);
            }
        }
    }
}
fn operands(mut response: Value) -> Result<Value, TestError> {
    let result = response
        .get_mut("result")
        .and_then(Value::as_object_mut)
        .ok_or("public work result is not an object")?;
    // The actual native feed head legitimately advances when facts are added.
    result.remove("asOf");
    result.remove("replicatedThrough");
    Ok(response)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn public_work_operands_and_native_queries_ignore_growing_unrelated_facts()
-> Result<(), TestError> {
    let statements = Arc::new(Mutex::new(Vec::new()));
    tracing_subscriber::registry()
        .with(Statements(statements.clone()))
        .init();
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    fixture.task(&project, &workstream).await?;
    let request = json!({"projectId":project});
    let expected = operands(fixture.api.call("view_work", request.clone()).await?)?;
    assert!(
        expected.pointer("/result/workstreams/0").is_some(),
        "real work operands missing: {expected}"
    );
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("native store")?;
    let mut read = store.snapshot().await?;
    let template = read
        .record(&project.clone().into())
        .await?
        .ok_or("project record")?;
    read.finish().await?;
    let RecordPlacement::Project(entry) = &template.envelope().position else {
        return Err("project origin".into());
    };
    for size in [8, 128] {
        let mut write = store.begin_admission(entry.origin.clone()).await?;
        for number in 0..size {
            let fact_id = FactId::generate();
            write
                .append_record(Record::Fact(Fact {
                    envelope: RecordEnvelope {
                        id: fact_id.clone().into(),
                        request: None,
                        ..template.envelope().clone()
                    },
                    fact_id,
                    kind: FactKind::EvaluatorResult {
                        guard: format!("unrelated {number}"),
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
                    links: Vec::new(),
                    producer_time: template
                        .envelope()
                        .time
                        .as_router_time()
                        .ok_or("router time")?,
                    anchor: None,
                    late: false,
                }))
                .await?;
        }
        write.commit().await?;
        let before = statements.lock().unwrap().len();
        let actual = fixture.api.call("view_work", request.clone()).await?;
        let selected = statements
            .lock()
            .unwrap()
            .iter()
            .skip(before)
            .cloned()
            .collect::<Vec<_>>();
        let normalized = selected
            .iter()
            .map(|sql| {
                sql.split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase()
            })
            .collect::<Vec<_>>();
        assert!(
            normalized.iter().any(|sql| sql.contains("from tasks")),
            "native SQL observer saw no task query: {selected:?}"
        );
        assert!(
            !normalized
                .iter()
                .any(|sql| sql.contains("rejectedtail") || sql.contains("from facts")),
            "view_work read unreturned fact history: {selected:?}"
        );
        assert_eq!(
            operands(actual)?,
            expected,
            "unrelated fact rows changed public work operands"
        );
        eprintln!(
            "public work view with added {size} fact rows: {} real native statements, no fact selection",
            selected.len()
        );
    }
    drop(store);
    fixture.stop().await
}
