use crate::*;
use fleet_admission::{FleetIds, FleetOperation, FleetRequest};
use message_board::*;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn request(
    workspace: WorkspaceId,
) -> Result<FleetRequest, Box<dyn std::error::Error + Send + Sync>> {
    Ok(FleetRequest {
        request_id: RequestId::generate(),
        workspace,
        actor: RecordedActor {
            actor: message_board::ActorKind::Declared(Identity::Human {
                human_id: HumanId::try_from("owner".to_owned())?,
            }),
            installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
            machine: MachineLabel::try_from("local fixture".to_owned())?,
        },
        acting_for: None,
        resubmits: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: FleetOperation::WorkspaceCreate {
            name: ResourceName::try_from("Local".to_owned())?,
            description: Description::try_from("local stores".to_owned())?,
        },
    })
}

#[tokio::test]
async fn pending_fleet_requests_keep_their_ids_and_decisions_across_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("outbox.sqlite");
    let outbox = FleetOutbox::open(&path).await?;
    let original = request(WorkspaceId::generate())?;
    let ids = FleetIds {
        generated_record: RecordId::generate(),
    };
    let first = outbox.enqueue(&original, &ids).await?;
    let different_ids = FleetIds {
        generated_record: RecordId::generate(),
    };
    assert_eq!(
        outbox.enqueue(&original, &different_ids).await?,
        first,
        "retry uses the first supplied record ID"
    );
    let mut conflicting = original.clone();
    conflicting.operation = FleetOperation::WorkspaceCreate {
        name: ResourceName::try_from("different".to_owned())?,
        description: Description::try_from("local stores".to_owned())?,
    };
    assert!(matches!(
        outbox.enqueue(&conflicting, &ids).await,
        Err(RouterLocalStorageError::ConflictingRequest)
    ));
    outbox.close().await;
    let reopened = FleetOutbox::open(&path).await?;
    assert_eq!(reopened.pending().await?, vec![first]);
    let rejected = RequestDecision::Rejected {
        reasons: vec![Refusal::Archived],
    };
    reopened
        .mark_decided(&original.request_id, &rejected)
        .await?;
    reopened
        .mark_decided(&original.request_id, &rejected)
        .await?;
    assert!(reopened.pending().await?.is_empty());
    assert!(matches!(
        reopened
            .mark_decided(
                &original.request_id,
                &RequestDecision::Rejected {
                    reasons: vec![Refusal::DefinitionConflict]
                }
            )
            .await,
        Err(RouterLocalStorageError::ConflictingRequest)
    ));
    let kept = reopened.enqueue(&original, &different_ids).await?;
    assert_eq!(kept.ids, ids);
    assert_eq!(kept.state, FleetOutboxState::Decided { decision: rejected });
    reopened.close().await;
    Ok(())
}

#[tokio::test]
async fn fleet_cache_installs_a_real_snapshot_and_rejects_a_gapped_page_atomically() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fleet = FleetStore::open(&directory.path().join("fleet.sqlite")).await?;
    let cache_path = directory.path().join("cache.sqlite");
    let cache = FleetCache::open(&cache_path).await?;
    let original = request(WorkspaceId::generate())?;
    fleet
        .admit(
            &original,
            "2026-10-12T00:00:00Z".parse()?,
            &FleetIds {
                generated_record: RecordId::generate(),
            },
        )
        .await?;
    let snapshot = fleet.snapshot_records().await?;
    cache.replace_snapshot(&snapshot).await?;
    assert_eq!(cache.snapshot().await?, snapshot);
    let Some(mut record) = cache.record(&original.workspace.clone().into()).await? else {
        panic!("workspace missing")
    };
    let gap = FleetPosition::try_from(snapshot.through.get() + 2)?;
    record.envelope_mut().position = RecordPosition::Fleet(gap);
    assert!(matches!(
        cache
            .apply_feed(
                snapshot.through,
                &[fleet_admission::FleetFeedEntry {
                    position: gap,
                    record
                }]
            )
            .await,
        Err(RouterLocalStorageError::PositionGap)
    ));
    assert_eq!(cache.snapshot().await?, snapshot);
    let mut rename = original.clone();
    rename.request_id = RequestId::generate();
    rename.operation = FleetOperation::WorkspaceUpdate {
        name: ResourceName::try_from("Updated through feed".to_owned())?,
        description: Description::try_from("local stores".to_owned())?,
    };
    fleet
        .admit(
            &rename,
            "2026-10-12T00:00:00Z".parse()?,
            &FleetIds {
                generated_record: RecordId::generate(),
            },
        )
        .await?;
    let page = fleet.feed_after(snapshot.through, 1).await?;
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.covered_through.get(), 3);
    assert_eq!(page.high_water.get(), 4);
    cache.apply_feed(snapshot.through, &page.entries).await?;
    assert_eq!(cache.snapshot().await?.through, page.covered_through);
    let remainder = fleet.feed_after(page.covered_through, 10).await?;
    cache
        .apply_feed(page.covered_through, &remainder.entries)
        .await?;
    assert_eq!(cache.snapshot().await?, fleet.snapshot_records().await?);
    cache.close().await;
    let reopened = FleetCache::open(&cache_path).await?;
    assert_eq!(reopened.snapshot().await?, fleet.snapshot_records().await?);
    reopened.close().await;
    fleet.close().await;
    Ok(())
}

#[tokio::test]
async fn legacy_bounds_translate_each_project_and_keep_empty_scopes_and_key() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("legacy.sqlite");
    let store = CursorStore::open(&path).await?;
    let first = ProjectId::generate();
    let second = ProjectId::generate();
    let empty = WorkspaceId::generate();
    let epoch = EpochNumber::try_from(1)?;
    let mapping = vec![
        LegacyPosition {
            old: ActivitySequence::try_from(4)?,
            project: first.clone(),
            cursor: FeedCursor {
                epoch,
                position: FeedPosition::try_from(1)?,
            },
        },
        LegacyPosition {
            old: ActivitySequence::try_from(8)?,
            project: second.clone(),
            cursor: FeedCursor {
                epoch,
                position: FeedPosition::try_from(1)?,
            },
        },
        LegacyPosition {
            old: ActivitySequence::try_from(11)?,
            project: first.clone(),
            cursor: FeedCursor {
                epoch,
                position: FeedPosition::try_from(2)?,
            },
        },
    ];
    let scopes = vec![
        LegacyScopeTranslation {
            scope: LegacyCursorScope::AllProjects {},
            projects: vec![first.clone(), second.clone()],
        },
        LegacyScopeTranslation {
            scope: LegacyCursorScope::Project(first.clone()),
            projects: vec![first.clone()],
        },
        LegacyScopeTranslation {
            scope: LegacyCursorScope::Workspace(empty.clone()),
            projects: Vec::new(),
        },
    ];
    let key = CursorSigningKey::try_from(vec![7; 32])?;
    store.install(&key, &mapping, &scopes).await?;
    store.close().await;
    let reopened = CursorStore::open(&path).await?;
    assert_eq!(reopened.key().await?, Some(key));
    let translated = reopened
        .translate(
            &LegacyCursorScope::AllProjects {},
            ActivitySequence::try_from(9)?,
        )
        .await?;
    assert_eq!(
        translated
            .iter()
            .find(|(id, _)| id == &first)
            .map(|(_, cursor)| cursor.position.get()),
        Some(1)
    );
    assert_eq!(
        translated
            .iter()
            .find(|(id, _)| id == &second)
            .map(|(_, cursor)| cursor.position.get()),
        Some(1)
    );
    assert_eq!(
        reopened
            .translate(
                &LegacyCursorScope::Project(first),
                ActivitySequence::try_from(11)?
            )
            .await?
            .first()
            .map(|(_, cursor)| cursor.position.get()),
        Some(2)
    );
    assert!(
        reopened
            .translate(
                &LegacyCursorScope::Workspace(empty),
                ActivitySequence::try_from(9)?
            )
            .await?
            .is_empty()
    );
    assert!(matches!(
        reopened
            .translate(
                &LegacyCursorScope::Board(BoardId::generate()),
                ActivitySequence::try_from(9)?
            )
            .await,
        Err(RouterLocalStorageError::InvalidCursor)
    ));
    reopened.close().await;
    Ok(())
}

#[tokio::test]
async fn repository_attachments_travel_in_workspace_feed_cache_and_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fleet_path = directory.path().join("fleet.sqlite");
    let cache_path = directory.path().join("cache.sqlite");
    let fleet = FleetStore::open(&fleet_path).await?;
    let cache = FleetCache::open(&cache_path).await?;
    let original = request(WorkspaceId::generate())?;
    let now = "2026-10-12T00:00:00Z".parse()?;
    let ids = FleetIds {
        generated_record: RecordId::generate(),
    };
    fleet.admit(&original, now, &ids).await?;
    cache
        .replace_snapshot(&fleet.snapshot_records().await?)
        .await?;
    let repository: RepositoryRef = serde_json::from_value(serde_json::json!({
        "kind":"origin", "normalizedOrigin":"github.com/example/repository"
    }))?;
    for attached in [true, false] {
        let mut mutation = original.clone();
        mutation.request_id = RequestId::generate();
        mutation.operation = if attached {
            FleetOperation::RepositoryAttach {
                repository: repository.clone(),
            }
        } else {
            FleetOperation::RepositoryDetach {
                repository: repository.clone(),
            }
        };
        let after = cache.snapshot().await?.through;
        let reply = fleet.admit(&mutation, now, &ids).await?;
        let expected = if attached {
            vec![repository.clone()]
        } else {
            Vec::new()
        };
        let workspace = fleet
            .record(&original.workspace.clone().into())
            .await?
            .ok_or("workspace missing")?;
        let payload = serde_json::to_value(&workspace)?;
        assert_eq!(
            payload["record"]["repositories"],
            serde_json::to_value(&expected)?
        );
        assert!(reply.records.contains(&workspace));
        let page = fleet.feed_after(after, 100).await?;
        assert!(page.entries.iter().any(|entry| entry.record == workspace));
        cache.apply_feed(after, &page.entries).await?;
        assert_eq!(
            cache.record(&original.workspace.clone().into()).await?,
            Some(workspace)
        );
        let before_replay = fleet.snapshot_records().await?;
        assert_eq!(fleet.admit(&mutation, now, &ids).await?, reply);
        assert_eq!(fleet.snapshot_records().await?, before_replay);
    }
    cache.close().await;
    fleet.close().await;
    let reopened_fleet = FleetStore::open(&fleet_path).await?;
    let reopened_cache = FleetCache::open(&cache_path).await?;
    assert_eq!(
        reopened_cache.snapshot().await?,
        reopened_fleet.snapshot_records().await?
    );
    reopened_cache.close().await;
    reopened_fleet.close().await;
    Ok(())
}

#[tokio::test]
async fn cursor_key_is_stable_and_signed_payloads_refuse_tampering() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("cursor.sqlite");
    let store = CursorStore::open(&path).await?;
    let key = CursorSigningKey::try_from(vec![9; 32])?;
    assert_eq!(store.get_or_install_key(&key).await?, key);
    store.close().await;
    let reopened = CursorStore::open(&path).await?;
    assert_eq!(
        reopened
            .get_or_install_key(&CursorSigningKey::try_from(vec![8; 32])?)
            .await?,
        key
    );
    let cursor = encode_board_cursor(
        &key,
        &serde_json::json!({"operation":"post_list","through":{"epoch":1,"position":42}}),
    )?;
    let payload: serde_json::Value = decode_board_cursor(&key, &cursor)?;
    assert_eq!(payload["through"]["position"], 42);
    let mut forged = cursor.into_bytes();
    forged[0] = if forged[0] == b'A' { b'B' } else { b'A' };
    assert!(decode_board_cursor::<serde_json::Value>(&key, &String::from_utf8(forged)?).is_err());
    reopened.close().await;
    Ok(())
}
