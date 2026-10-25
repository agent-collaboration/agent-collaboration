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

#[tokio::test]
async fn pending_fleet_requests_keep_their_ids_and_decisions_across_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("outbox.sqlite");
    let outbox = FleetOutbox::open(&path).await?;
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
                    reasons: vec![Refusal::InvalidShape {
                        field: "fixture".into()
                    }]
                }
            )
            .await,
        Err(RouterLocalStorageError::ConflictingRequest)
    ));
    let kept = reopened.enqueue(&original).await?;
    assert_eq!(kept.request, original);
    assert_eq!(kept.state, FleetOutboxState::Decided { decision: rejected });
    reopened.close().await;
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
