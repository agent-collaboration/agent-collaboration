//! Full-capture state updates preserve both images on product-sized intent rows.
use crate::ProjectStore;
use message_board::*;
use sqlx::{ConnectOptions, Connection, Row};
type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[test]
fn capturing_project_statements_do_not_use_conflict_updates() {
    for (name, source) in [
        ("project_record_io.rs", include_str!("project_record_io.rs")),
        (
            "project_reader_mark_io.rs",
            include_str!("project_reader_mark_io.rs"),
        ),
        (
            "project_record_relations.rs",
            include_str!("project_record_relations.rs"),
        ),
        ("project_watch_io.rs", include_str!("project_watch_io.rs")),
        (
            "project_participant_io.rs",
            include_str!("project_participant_io.rs"),
        ),
    ] {
        assert!(
            !source
                .split_whitespace()
                .collect::<Vec<_>>()
                .windows(2)
                .any(|words| words == ["DO", "UPDATE"]),
            "capturing conflict update in {name}"
        );
    }
}

#[tokio::test]
async fn product_sized_intent_state_change_captures_emitted_before_and_handed_off_after()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("intent-images.sqlite");
    let project = ProjectId::generate();
    let store = ProjectStore::open(&path, project.clone()).await?;
    let base = crate::project_store_tests::project_record(project.clone());
    let mut intents = Vec::new();
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    write.append_record(base.clone()).await?;
    for index in 0..6 {
        let id = NotifyIntentId::generate();
        let actor = RecordedActor::declared(Identity::Session {
            session: SessionRef {
                endpoint: SessionEndpointRef {
                    service_id: crate::project_test_support::origin().installation,
                    endpoint_id: EndpointId::try_from("codex-local".to_owned())?,
                },
                session_id: SessionId::try_from(format!(
                    "session-{index}-{}",
                    "recipient-context-".repeat(24)
                ))?,
            },
        });
        let intent = NotifyIntent::new(NotifyIntentProps {
            envelope: RecordEnvelope {
                id: id.clone().into(),
                ..base.envelope().clone()
            },
            intent_id: id,
            cause: project.clone().into(),
            recipient: IntentRecipient::Resolved(ResolvedRecipient {
                actor,
                via_role: Some(RoleRecipient {
                    subject: RoleSubject::Project(project.clone()),
                    role: AuthorityRole::Lead,
                    assignment_revision: AssignmentRevision::try_from(1)?,
                }),
            }),
            state: IntentState::Emitted {},
        })?;
        write
            .append_record(Record::NotifyIntent(intent.clone()))
            .await?;
        intents.push(intent);
    }
    write.commit().await?;
    let mut changed = intents.first().ok_or("intent")?.clone();
    let item = MailItemRef::try_from(RecordId::generate().as_str().to_owned())?;
    changed.set_state(IntentState::HandedOff { item: item.clone() })?;
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    let assigned = write
        .append_record(Record::NotifyIntent(changed.clone()))
        .await?;
    changed.envelope.position = crate::project_test_support::placement(assigned);
    write.commit().await?;
    let mut read = store.snapshot().await?;
    assert_eq!(
        read.record(&changed.intent_id.clone().into()).await?,
        Some(Record::NotifyIntent(changed.clone()))
    );
    read.finish().await?;
    let mut capture = TursoConnectOptions::new()
        .filename(&path)
        .create_if_missing(false)
        .connect()
        .await?;
    let rows=sqlx::query::<sqlx_turso::Turso>("SELECT bin_record_json_object(table_columns_json_array('notify_intents'), before) AS before_state, bin_record_json_object(table_columns_json_array('notify_intents'), after) AS after_state FROM turso_cdc WHERE table_name = 'notify_intents' AND before IS NOT NULL ORDER BY change_id").fetch_all(&mut capture).await?;
    assert_eq!(
        rows.len(),
        1,
        "one changed intent produces one captured update"
    );
    let before: serde_json::Value =
        serde_json::from_str(&rows[0].try_get::<String, _>("before_state")?)?;
    let after: serde_json::Value =
        serde_json::from_str(&rows[0].try_get::<String, _>("after_state")?)?;
    assert_eq!(before["intent_id"], serde_json::json!(changed.intent_id));
    assert_eq!(after["intent_id"], serde_json::json!(changed.intent_id));
    assert_eq!(
        before["state"],
        serde_json::json!(r#"{"kind":"emitted"}"#),
        "{before}"
    );
    assert_eq!(
        after["state"],
        serde_json::json!(format!(
            r#"{{"kind":"handedOff","item":"{}"}}"#,
            item.as_str()
        )),
        "{after}"
    );
    capture.close().await?;
    store.close().await;
    Ok(())
}
use sqlx_turso::TursoConnectOptions;
