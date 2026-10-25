//! Valid upgrade input and the original invalid all-table sentinel, with preservation checks.
use super::*;

#[derive(Clone, Copy)]
pub(super) enum WakeFixture {
    Valid,
    Opaque,
    OpaqueDefinition,
}
impl WakeFixture {
    fn id(self) -> &'static str {
        match self {
            Self::Valid | Self::OpaqueDefinition => "01900000-0000-7000-8000-000000000001",
            Self::Opaque => "wake-1",
        }
    }
    fn definition(self, upgraded: bool) -> serde_json::Value {
        if !matches!(self, Self::Valid) {
            return serde_json::json!({"message":"kept"});
        }
        let mut value = serde_json::json!({
            "wakeupId":self.id(),"changeId":"01900000-0000-7000-8000-000000000002",
            "message":{"target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"legacy-fixture"},"content":{"kind":"humanUser","text":"kept"},"delivery":"queue"},
            "timing":{"kind":"at","at":"1970-01-01T00:00:00.114Z"},"anchorAtMs":112,"expiresAtMs":115,"createdAtMs":116
        });
        if let Some(object) = value.as_object_mut().filter(|_| upgraded) {
            object.insert("createdBy".into(), serde_json::json!({"kind":"unrecorded"}));
        }
        value
    }
}

pub(super) async fn seed_all_domain_tables(
    path: &Path,
    wake_fixture: WakeFixture,
) -> Result<(), sqlx::Error> {
    let mut connection = connect(path).await?;
    let mut transaction = connection.begin().await?;
    sqlx::query("INSERT INTO instruction_documents VALUES ('instruction-1','revision-1','instruction bytes',101)").execute(&mut *transaction).await?;
    sqlx::query("INSERT INTO instruction_revisions VALUES ('revision-1','instruction-1','instruction bytes','source-revision',102)").execute(&mut *transaction).await?;
    sqlx::query("INSERT INTO schedule_definitions VALUES ('schedule-1','change-1','instruction-1',1,'{\"timing\":\"kept\"}','{\"summary\":null}',103,104)").execute(&mut *transaction).await?;
    sqlx::query("INSERT INTO schedule_timing_state VALUES ('schedule-1','change-1',105,106,107)")
        .execute(&mut *transaction)
        .await?;
    sqlx::query("INSERT INTO thread_bindings VALUES ('binding-1','schedule-1','service-1','endpoint-1','thread-1',108)").execute(&mut *transaction).await?;
    sqlx::query("INSERT INTO workflow_runs VALUES ('run-1','schedule-1',109,'uncertain','{\"input\":\"kept\"}','binding-1','turn-1',110,111,3600,'{\"nativeInputAccepted\":true}','{\"status\":\"kept\"}','{\"attempt\":1}',NULL,NULL,NULL)").execute(&mut *transaction).await?;
    let wake_id = wake_fixture.id();
    let definition = wake_fixture.definition(false).to_string();
    sqlx::query("INSERT INTO wakeup_definitions VALUES (?,?,'active',?,112,113,114,115,'{\"fired\":true}',NULL,116,117)")
        .bind(wake_id).bind("01900000-0000-7000-8000-000000000002").bind(definition).execute(&mut *transaction).await?;
    sqlx::query("INSERT INTO mailbox_deliveries VALUES ('delivery-1',?,'occurrence-1',118,119,'{\"thread\":\"kept\"}','{\"text\":\"kept\"}','queue','{\"generation\":7}','uncertain',120,121,'{\"attempt\":2}',NULL,122)")
        .bind(wake_id).execute(&mut *transaction).await?;
    sqlx::query("UPDATE wakeup_definitions SET pending_delivery_id='delivery-1' WHERE wakeup_id=?")
        .bind(wake_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("INSERT INTO operation_receipts VALUES ('operation-1','submit',x'0001FF','run-1','uncertain','{\"effect\":\"kept\"}',NULL,NULL,123)").execute(&mut *transaction).await?;
    sqlx::query("INSERT INTO automation_events(event_sequence,event_id,subject_kind,subject_id,event_kind,event_body_json,recorded_at_ms) VALUES (41,'event-1','run','run-1','uncertain','{\"evidence\":\"kept\"}',124)").execute(&mut *transaction).await?;
    sqlx::query("INSERT INTO automation_events(event_sequence,event_id,subject_kind,subject_id,event_kind,event_body_json,recorded_at_ms) VALUES (79,'event-high-water','run','run-1','temporary','{}',125)").execute(&mut *transaction).await?;
    sqlx::query("DELETE FROM automation_events WHERE event_id='event-high-water'")
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    connection.close().await
}

pub(super) async fn assert_preserved_domain_state(
    path: &Path,
    wake_fixture: WakeFixture,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut connection = connect(path).await?;
    let instruction: (String, String, i64) = sqlx::query_as(
        "SELECT current_revision_id,instruction_text,updated_at_ms FROM instruction_documents WHERE instruction_id='instruction-1'",
    ).fetch_one(&mut connection).await?;
    ensure_eq!(
        instruction,
        ("revision-1".into(), "instruction bytes".into(), 101)
    );
    let revision: (String, Option<String>, i64) = sqlx::query_as("SELECT instruction_text,source_revision_id,recorded_at_ms FROM instruction_revisions WHERE revision_id='revision-1'").fetch_one(&mut connection).await?;
    ensure_eq!(
        revision,
        (
            "instruction bytes".into(),
            Some("source-revision".into()),
            102
        )
    );
    let schedule: (i64, String, String) = sqlx::query_as("SELECT enabled,definition_json,imported_continuity_json FROM schedule_definitions WHERE schedule_id='schedule-1'").fetch_one(&mut connection).await?;
    ensure_eq!(
        schedule,
        (
            1,
            "{\"timing\":\"kept\"}".into(),
            "{\"summary\":null}".into()
        )
    );
    let timing: (i64, i64, Option<i64>) = sqlx::query_as("SELECT anchor_at_ms,evaluated_through_ms,next_due_at_ms FROM schedule_timing_state WHERE schedule_id='schedule-1'").fetch_one(&mut connection).await?;
    ensure_eq!(timing, (105, 106, Some(107)));
    let binding: (String, String, String) = sqlx::query_as("SELECT service_id,endpoint_id,thread_id FROM thread_bindings WHERE thread_binding_id='binding-1'").fetch_one(&mut connection).await?;
    ensure_eq!(
        binding,
        ("service-1".into(), "endpoint-1".into(), "thread-1".into())
    );
    let run: (String, Option<String>, String, Option<String>) = sqlx::query_as(
        "SELECT run_status,captured_inputs_json,execution_evidence_json,worker_outcome_json FROM workflow_runs WHERE run_id='run-1'",
    ).fetch_one(&mut connection).await?;
    ensure_eq!(
        run,
        (
            "uncertain".into(),
            Some("{\"input\":\"kept\"}".into()),
            "{\"nativeInputAccepted\":true}".into(),
            Some("{\"status\":\"kept\"}".into())
        )
    );
    let wake: (String, String, Option<String>) = sqlx::query_as("SELECT wakeup_status,definition_json,pending_delivery_id FROM wakeup_definitions WHERE wakeup_id=?").bind(wake_fixture.id()).fetch_one(&mut connection).await?;
    ensure_eq!(wake.0, "active");
    // MS-10 adds Unrecorded creator metadata while preserving every original field.
    ensure_eq!(
        serde_json::from_str::<serde_json::Value>(&wake.1)?,
        wake_fixture.definition(matches!(wake_fixture, WakeFixture::Valid))
    );
    ensure_eq!(wake.2, Some("delivery-1".into()));
    let delivery: (String, String, Option<String>) = sqlx::query_as("SELECT delivery_status,message_json,latest_attempt_json FROM mailbox_deliveries WHERE delivery_id='delivery-1'").fetch_one(&mut connection).await?;
    ensure_eq!(
        delivery,
        (
            "uncertain".into(),
            "{\"text\":\"kept\"}".into(),
            Some("{\"attempt\":2}".into())
        )
    );
    let receipt: (Vec<u8>, String, String) = sqlx::query_as(
        "SELECT canonical_request,operation_status,effect_evidence_json FROM operation_receipts WHERE operation_id='operation-1'",
    ).fetch_one(&mut connection).await?;
    ensure_eq!(
        receipt,
        (
            vec![0, 1, 255],
            "uncertain".into(),
            "{\"effect\":\"kept\"}".into()
        )
    );
    let event: (i64, String) = sqlx::query_as(
        "SELECT event_sequence,event_body_json FROM automation_events WHERE event_id='event-1'",
    )
    .fetch_one(&mut connection)
    .await?;
    ensure_eq!(event, (41, "{\"evidence\":\"kept\"}".into()));
    let high_water: i64 =
        sqlx::query_scalar("SELECT seq FROM sqlite_sequence WHERE name='automation_events'")
            .fetch_one(&mut connection)
            .await?;
    ensure_eq!(high_water, 79);
    let foreign_key_failures = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&mut connection)
        .await?;
    ensure!(foreign_key_failures.is_empty());
    connection.close().await?;
    Ok(())
}
