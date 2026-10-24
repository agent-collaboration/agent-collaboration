use super::*;
use fleet_admission::FleetOperation;
use message_board::*;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn inputs()
-> Result<(ProjectCreateRequest, FleetRequest, FleetIds), Box<dyn std::error::Error + Send + Sync>>
{
    let input = ProjectCreateRequest {
        request_id: RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: ProjectId::generate(),
        board_id: BoardId::generate(),
        title: ResourceName::try_from("queued creation".to_owned())?,
        description: Some(Description::try_from(" original\ndescription ".to_owned())?),
        actor: Identity::Human {
            human_id: HumanId::try_from("owner".to_owned())?,
        },
        acting_for: None,
    };
    let directory = FleetRequest {
        request_id: input.request_id.clone(),
        workspace: WorkspaceId::generate(),
        actor: RecordedActor::declared(input.actor.clone()),
        acting_for: input.acting_for.clone(),
        protocol_version: input.protocol_version,
        operation: FleetOperation::ProjectDirectoryCreate {
            project: input.project_id.clone(),
            board: input.board_id.clone(),
            title: input.title.clone(),
        },
    };
    Ok((
        input,
        directory,
        FleetIds {
            generated_record: RecordId::generate(),
        },
    ))
}

#[tokio::test]
async fn complete_creation_input_and_identity_survive_pending_conflict_and_reopen() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("outbox.sqlite");
    let outbox = FleetOutbox::open(&path).await?;
    let (input, directory, ids) = inputs()?;
    let queued = outbox.enqueue_creation(&directory, &ids, &input).await?;
    let mut conflicting = input.clone();
    conflicting.description = Some(Description::try_from("changed".to_owned())?);
    assert!(matches!(
        outbox
            .enqueue_creation(&directory, &ids, &conflicting)
            .await,
        Err(RouterLocalStorageError::ConflictingRequest)
    ));
    assert_eq!(
        outbox.creation_input(&input.request_id).await?,
        Some(input.clone())
    );
    assert!(outbox.accepted_creation_page("", 100).await?.is_empty());
    outbox.close().await;
    let outbox = FleetOutbox::open(&path).await?;
    assert_eq!(outbox.request(&input.request_id).await?, Some(queued));
    assert_eq!(
        outbox.creation_input(&input.request_id).await?,
        Some(input.clone())
    );
    let accepted = RequestDecision::Accepted {
        effects: DecisionEffectsPage::from_full(
            Vec::new(),
            &DecisionEffectsHeader::from_full(&[]),
            None,
        )?,
        change: ChangeOutcome::Changed,
    };
    outbox.mark_decided(&input.request_id, &accepted).await?;
    assert_eq!(outbox.accepted_creation_page("", 100).await?, vec![input]);
    outbox.close().await;
    Ok(())
}

#[tokio::test]
async fn creation_input_failure_rolls_back_its_directory_request() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let outbox = FleetOutbox::open(&temporary.path().join("outbox.sqlite")).await?;
    let (input, directory, ids) = inputs()?;
    // A fault at the second table must not leave the first half enqueued.
    sqlx::query("DROP TABLE project_creation_inputs")
        .execute(&outbox.database.pool)
        .await?;
    assert!(
        outbox
            .enqueue_creation(&directory, &ids, &input)
            .await
            .is_err()
    );
    assert!(outbox.request(&input.request_id).await?.is_none());
    outbox.close().await;
    Ok(())
}

#[tokio::test]
async fn authoritative_creation_decode_rejects_changed_input_and_pair_identity() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let outbox = FleetOutbox::open(&temporary.path().join("outbox.sqlite")).await?;
    let (input, directory, ids) = inputs()?;
    outbox.enqueue_creation(&directory, &ids, &input).await?;
    let mut changed = input.clone();
    changed.description = Some(Description::try_from("corrupt description".to_owned())?);
    sqlx::query("UPDATE project_creation_inputs SET input = ? WHERE request_id = ?")
        .bind(serde_json::to_string(&changed)?)
        .bind(input.request_id.as_str())
        .execute(&outbox.database.pool)
        .await?;
    assert!(matches!(
        outbox.creation_input(&input.request_id).await,
        Err(RouterLocalStorageError::InvalidRecord)
    ));
    changed = input.clone();
    changed.project_id = ProjectId::generate();
    let hash = request_canonical::creation_hash(&changed)?;
    sqlx::query(
        "UPDATE project_creation_inputs SET input = ?, canonical_hash = ? WHERE request_id = ?",
    )
    .bind(serde_json::to_string(&changed)?)
    .bind(hash.as_str())
    .bind(input.request_id.as_str())
    .execute(&outbox.database.pool)
    .await?;
    assert!(matches!(
        outbox.creation_input(&input.request_id).await,
        Err(RouterLocalStorageError::InvalidRecord)
    ));
    outbox.close().await;
    Ok(())
}
