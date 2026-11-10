use crate::*;
use fleet_admission::{FleetOperation, FleetRequest};
use message_board::*;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn request(
    workspace: WorkspaceId,
) -> Result<FleetRequest, Box<dyn std::error::Error + Send + Sync>> {
    Ok(FleetRequest {
        request_id: RequestId::generate(),
        workspace,
        actor: RecordedActor::declared(Identity::Human {
            human_id: HumanId::try_from("owner".to_owned())?,
        }),
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: FleetOperation::WorkspaceCreate {
            name: ResourceName::try_from("Local".to_owned())?,
            description: Description::try_from("local stores".to_owned())?,
        },
    })
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn pending_fleet_requests_keep_their_ids_and_decisions_across_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("outbox.sqlite");
    let outbox_store = crate::FleetStore::open(&path).await?;
    let outbox = outbox_store.outbox();
    let original = request(WorkspaceId::generate())?;
    let first = outbox.enqueue(&original).await?;
    assert_eq!(
        outbox.enqueue(&original).await?,
        first,
        "retry keeps the original request"
    );
    let mut conflicting = original.clone();
    conflicting.operation = FleetOperation::WorkspaceCreate {
        name: ResourceName::try_from("different".to_owned())?,
        description: Description::try_from("local stores".to_owned())?,
    };
    assert!(matches!(
        outbox.enqueue(&conflicting).await,
        Err(RouterLocalStorageError::ConflictingRequest)
    ));
    outbox_store.close().await;
    let reopened_store = crate::FleetStore::open(&path).await?;
    let reopened = reopened_store.outbox();
    assert_eq!(reopened.pending().await?, vec![first]);
    let rejected = RequestDecision::Rejected {
        reasons: vec![Refusal::Archived],
    };
    reopened
        .mark_decided(&original.request_id, &rejected, 1_000)
        .await?;
    reopened
        .mark_decided(&original.request_id, &rejected, 1_000)
        .await?;
    assert!(reopened.pending().await?.is_empty());
    assert!(matches!(
        reopened
            .mark_decided(
                &original.request_id,
                &RequestDecision::Rejected {
                    reasons: vec![Refusal::InvalidShape {
                        field: "fixture".into()
                    }]
                },
                2_000,
            )
            .await,
        Err(RouterLocalStorageError::ConflictingRequest)
    ));
    let kept = reopened.enqueue(&original).await?;
    assert_eq!(kept.request, original);
    assert_eq!(kept.state, FleetOutboxState::Decided { decision: rejected });
    reopened_store.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn cursor_key_is_stable_and_signed_payloads_refuse_tampering() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("cursor.sqlite");
    let store_store = crate::FleetStore::open(&path).await?;
    let store = store_store.cursors();
    let key = CursorSigningKey::try_from(vec![9; 32])?;
    assert_eq!(store.get_or_install_key(&key).await?, key);
    store_store.close().await;
    let reopened_store = crate::FleetStore::open(&path).await?;
    let reopened = reopened_store.cursors();
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
    reopened_store.close().await;
    Ok(())
}

fn commit_bumped(commits: &mut tokio::sync::watch::Receiver<u64>) -> bool {
    let changed = commits.has_changed().unwrap_or(false);
    commits.borrow_and_update();
    changed
}

/// Q2 at the Fleet store: an enqueue and a decision signal; a replayed enqueue, reads,
/// a refused conflicting enqueue and an empty prune don't.
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn fleet_writes_signal_their_commit_and_reads_do_not() -> TestResult {
    let directory = tempfile::tempdir()?;
    let store = crate::FleetStore::open(&directory.path().join("fleet.sqlite")).await?;
    let outbox = store.outbox();
    let mut commits = outbox.subscribe_commits();
    let original = request(WorkspaceId::generate())?;

    outbox.enqueue(&original).await?;
    let enqueue_bumped = commit_bumped(&mut commits);
    outbox.enqueue(&original).await?;
    outbox.pending().await?;
    outbox.request(&original.request_id).await?;
    let mut conflicting = original.clone();
    conflicting.operation = FleetOperation::WorkspaceCreate {
        name: ResourceName::try_from("different".to_owned())?,
        description: Description::try_from("local stores".to_owned())?,
    };
    let refused = outbox.enqueue(&conflicting).await;
    let pruned = outbox.prune_expired(0, 16).await?;
    let quiet_bumped = commit_bumped(&mut commits);
    outbox
        .mark_decided(
            &original.request_id,
            &RequestDecision::Rejected {
                reasons: vec![Refusal::Archived],
            },
            1_000,
        )
        .await?;
    let decision_bumped = commit_bumped(&mut commits);

    assert!(enqueue_bumped, "a committed enqueue must signal");
    assert!(matches!(
        refused,
        Err(RouterLocalStorageError::ConflictingRequest)
    ));
    assert_eq!(pruned, 0);
    assert!(
        !quiet_bumped,
        "replay, reads, a refusal and an empty prune must not signal"
    );
    assert!(decision_bumped, "a committed decision must signal");
    store.close().await;
    Ok(())
}
