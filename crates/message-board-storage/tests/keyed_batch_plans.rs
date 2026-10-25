//! Key batches must seek each supplied record key on the actual Turso engine.
#![allow(clippy::panic_in_result_fn)]
use message_board_storage::PROJECT_MIGRATOR;
use sqlx::{ConnectOptions, Connection, Row};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

macro_rules! batch {
    ($name:literal, $key:literal) => {
        (
            $name,
            concat!(
                "EXPLAIN QUERY PLAN ",
                include_str!(concat!("../src/project_", $name, "_batch.sql"))
            ),
            $key,
        )
    };
}

#[tokio::test]
async fn every_keyed_record_batch_uses_an_index_search_instead_of_a_record_scan() -> TestResult {
    // Arrange: use the application's complete native schema, including its
    // existing primary keys and the redaction record-kind/record-ID index.
    let mut connection = sqlx_turso::TursoConnectOptions::new().connect().await?;
    PROJECT_MIGRATOR.run(&mut connection).await?;
    let batches = [
        batch!("state", "project_id"),
        batch!("workstream", "workstream_id"),
        batch!("task", "task_id"),
        batch!("subtask", "subtask_id"),
        batch!("task_link_event", "event_id"),
        batch!("thread", "thread_id"),
        batch!("post", "post_id"),
        batch!("refiling", "refiling_id"),
        batch!("role", "assignment_id"),
        batch!("grant", "grant_id"),
        batch!("stand_down_ack", "acknowledgement_id"),
        batch!("trail", "row_id"),
        batch!("fact", "fact_id"),
        batch!("plan_revision", "revision_id"),
        batch!("decision", "request_id"),
        batch!("intent", "intent_id"),
        batch!("membership", "membership_id"),
        batch!("redaction", "record_id"),
    ];
    let mut regressions = Vec::new();
    for (name, sql, key) in batches {
        // Act: explain the actual checked query with its unchanged public bind
        // order (project, JSON keys), rather than a scalar-query stand-in.
        let plan = sqlx::query(sql)
            .bind("synthetic-project")
            .bind(r#"["synthetic-record"]"#)
            .fetch_all(&mut connection)
            .await?
            .iter()
            .map(|row| row.try_get::<String, _>("detail"))
            .collect::<Result<Vec<_>, _>>()?;
        eprintln!("{name}: {}", plan.join("; "));

        // Assert: scans of json_each are expected; scans of the record relation
        // are not. Joined actors/feed rows cannot satisfy this assertion.
        let record_steps: Vec<_> = plan
            .iter()
            .filter(|step| {
                step.contains(" AS r ")
                    || step.starts_with("SEARCH r ")
                    || step.starts_with("SCAN r ")
            })
            .collect();
        let key_search = record_steps
            .iter()
            .any(|step| step.starts_with("SEARCH ") && step.contains(&format!("{key}=?")));
        let record_scan = record_steps.iter().any(|step| step.starts_with("SCAN "));
        if !key_search || record_scan {
            regressions.push(format!("{name}: {}", plan.join("; ")));
        }
    }
    connection.close().await?;
    assert!(
        regressions.is_empty(),
        "keyed batch regressions:\n{}",
        regressions.join("\n")
    );
    Ok(())
}
