use crate::ProjectStore;
use message_board::*;

#[tokio::test]
async fn cause_and_actor_key_rejects_a_second_identity_even_after_role_revision_changes()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("intent-actor-key.sqlite");
    let project = ProjectId::generate();
    let store = ProjectStore::open(&path, project.clone()).await?;
    let base = crate::project_store_tests::project_record(project.clone());
    let make_intent = |revision| {
        let id = NotifyIntentId::generate();
        NotifyIntent::new(NotifyIntentProps {
            envelope: RecordEnvelope {
                id: id.clone().into(),
                ..base.envelope().clone()
            },
            intent_id: id,
            cause: project.clone().into(),
            recipient: IntentRecipient::Resolved(ResolvedRecipient {
                actor: base.envelope().actor.clone(),
                via_role: Some(RoleRecipient {
                    subject: RoleSubject::Project(project.clone()),
                    role: AuthorityRole::Lead,
                    assignment_revision: AssignmentRevision::try_from(revision)?,
                }),
            }),
            state: IntentState::Emitted {},
        })
        .map_err(Box::<dyn std::error::Error + Send + Sync>::from)
    };
    let mut original = make_intent(1)?;
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    write.append_record(base.clone()).await?;
    let original_at = write
        .append_record(Record::NotifyIntent(original.clone()))
        .await?;
    original.envelope.position = crate::project_test_support::placement(original_at);
    write.commit().await?;
    let duplicate = make_intent(2)?;
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    assert!(
        write
            .append_record(Record::NotifyIntent(duplicate.clone()))
            .await
            .is_err(),
        "one cause/actor must have only one intent ID across role revisions"
    );
    write.rollback().await?;
    store.close().await;
    let store = ProjectStore::open(&path, project).await?;
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    assert_eq!(
        write.record(&original.intent_id.clone().into()).await?,
        Some(Record::NotifyIntent(original))
    );
    assert!(write.record(&duplicate.intent_id.into()).await?.is_none());
    write.rollback().await?;
    store.close().await;
    Ok(())
}
