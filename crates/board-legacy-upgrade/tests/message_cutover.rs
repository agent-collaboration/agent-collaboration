//! Four real tables cross owners; only A's migrator may finish the move.
#![allow(clippy::panic_in_result_fn)]
mod message_support;
use board_legacy_upgrade::{MessageCopyCounts, MessageUpgrade, MessageUpgradeReport};
use interaction_storage::InteractionStore;
use message_support::*;
use rusqlite::{Connection, params};

#[tokio::test]
async fn copy_cold_reopen_retry_keeps_first_ids_and_history_then_owning_drop_is_atomic()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("automation.sqlite");
    let destination = directory.path().join("interaction.sqlite");
    let push_ids = seeded_source(&source).await?;
    let original_history = history(&destination).await?;
    let original = std::fs::read(&source)?;
    assert!(matches!(
        automation_storage::AutomationStore::open(&source).await,
        Err(automation_storage::StorageError::MessageUpgradeRequired { .. })
    ));
    assert_eq!(std::fs::read(&source)?, original);
    let expected = MessageUpgradeReport::Verified {
        digest_basis: board_legacy_upgrade::MESSAGE_COPY_DIGEST_BASIS,
        counts: MessageCopyCounts {
            pushes: 2,
            subscription_progress: 1,
            intent_progress: 1,
            intent_cursors: 1,
        },
    };
    let prepared = MessageUpgrade::prepare(&source, &Default::default()).await?;
    assert_eq!(prepared.copy_to(&destination).await?, expected);
    assert_eq!(std::fs::read(&source)?, original);
    let ids = text_rows(&destination, IDS)?;
    assert_eq!(ids.len(), 2);
    assert!(
        ids.iter()
            .all(|id| uuid::Uuid::parse_str(id).is_ok_and(|id| id.get_version_num() == 7))
    );
    let owner = InteractionStore::open(&destination).await?;
    for id in &push_ids {
        let record = owner
            .get_push_record(&id.clone().try_into()?)
            .await?
            .ok_or("copied push missing")?;
        assert_eq!(record.body.as_deref(), Some("  preserved message\n"));
        assert_eq!(
            record
                .created_at
                .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
            "2026-10-21T12:34:56.123456789Z"
        );
    }
    owner.close().await?;
    assert_eq!(
        text_rows(
            &destination,
            "SELECT retention AS value FROM router_pushes ORDER BY push_id"
        )?,
        vec!["creationAge", "creationAge"]
    );
    assert_eq!(
        text_rows(
            &destination,
            "SELECT header_facts_json AS value FROM router_pushes ORDER BY push_id"
        )?,
        vec![" {\"kind\": \"directMessage\", \"senderDisplayName\":null} "; 2]
    );
    let payload = text_rows(
        &destination,
        "SELECT payload AS value FROM subscription_delivery_progress",
    )?
    .pop()
    .ok_or("subscription missing")?;
    let payload: serde_json::Value = serde_json::from_str(&payload)?;
    let window = payload.pointer("/roots/0/window").ok_or("window missing")?;
    for removed in ["heldSince", "nextRetryAt", "retryAttempts"] {
        assert!(window.get(removed).is_none());
    }
    assert_eq!(window.get("postCount"), Some(&serde_json::json!(2)));
    let retry = MessageUpgrade::prepare(&source, &Default::default()).await?;
    assert_eq!(retry.copy_to(&destination).await?, expected);
    assert_eq!(text_rows(&destination, IDS)?, ids);
    assert_eq!(text_rows(&destination, HISTORY)?, original_history);
    assert_eq!(retry.apply_drop(&destination).await?, expected);
    assert!(source_tables(&source)?.is_empty());
    assert_eq!(
        text_rows(
            &source,
            &format!(
                "SELECT CAST(count(*) AS TEXT) AS value FROM _sqlx_migrations WHERE version={DROP_VERSION} AND success=1"
            )
        )?,
        vec!["1"]
    );
    let completed = MessageUpgrade::prepare(&source, &Default::default()).await?;
    assert_eq!(
        completed.copy_to(&destination).await?,
        MessageUpgradeReport::AlreadyMoved
    );
    assert_eq!(
        completed.apply_drop(&destination).await?,
        MessageUpgradeReport::AlreadyMoved
    );
    automation_storage::AutomationStore::open(&source)
        .await?
        .close()
        .await?;
    assert_eq!(text_rows(&destination, IDS)?, ids);
    assert_eq!(text_rows(&destination, HISTORY)?, original_history);
    Ok(())
}

#[tokio::test]
async fn current_product_automation_store_is_already_moved_and_untouched() -> TestResult {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("automation.sqlite");
    let destination = directory.path().join("interaction.sqlite");
    automation_storage::AutomationStore::open(&source)
        .await?
        .close()
        .await?;
    let original = std::fs::read(&source)?;

    let upgrade = MessageUpgrade::prepare(&source, &Default::default()).await?;
    assert_eq!(
        upgrade.copy_to(&destination).await?,
        MessageUpgradeReport::AlreadyMoved
    );
    assert_eq!(
        upgrade.apply_drop(&destination).await?,
        MessageUpgradeReport::AlreadyMoved
    );
    assert_eq!(std::fs::read(&source)?, original);
    assert!(!destination.exists());
    Ok(())
}

#[tokio::test]
async fn changed_destination_refuses_drop_and_retry_never_overwrites_it() -> TestResult {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("automation.sqlite");
    let destination = directory.path().join("interaction.sqlite");
    seeded_source(&source).await?;
    let prepared = MessageUpgrade::prepare(&source, &Default::default()).await?;
    prepared.copy_to(&destination).await?;
    let original = std::fs::read(&source)?;
    {
        let connection = Connection::open(&destination)?;
        connection.execute(
            "UPDATE router_pushes SET body='different destination body'",
            [],
        )?;
    }
    assert!(prepared.apply_drop(&destination).await.is_err());
    assert!(prepared.copy_to(&destination).await.is_err());
    assert_eq!(std::fs::read(&source)?, original);
    assert_eq!(source_tables(&source)?.len(), 5);
    assert_eq!(
        text_rows(
            &destination,
            "SELECT body AS value FROM router_pushes ORDER BY push_id"
        )?,
        vec!["different destination body"; 2]
    );
    Ok(())
}

#[tokio::test]
async fn released_native_receipt_extensions_copy_exactly_and_read_through_the_strict_owner()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("automation.sqlite");
    let destination = directory.path().join("interaction.sqlite");
    let ids = seeded_source(&source).await?;
    let generation =
        serde_json::json!({"serviceEpoch":"550e8400-e29b-41d4-a716-446655440000","generation":1});
    let receipts = [
        serde_json::json!({"outcome":{"kind":"startedOrSteered"},"reachability":"codexAppServer","client":{
            "kind":"codexAppServer","target":{"endpoint":{"serviceId":"550e8400-e29b-41d4-a716-446655440000","endpointId":"codex-local"},"sessionId":"legacy-recipient"},
            "generation":generation,"inputKind":"agent","representation":"declaredAgentText","clientUserMessageId":"released-fixture-message","resumeEffect":"notRequested",
            "permissionDifferences":["profile","sandbox","writableRoots","network"],
            "acceptance":{"kind":"nativeInputAccepted","operation":"turnStart","disposition":"startedOrSteered","turnId":"released-fixture-turn"}
        }}),
        serde_json::json!({"outcome":{"kind":"rejected","reason":"localRejection","nextAction":"inspectTarget","clientCode":null,"detail":null,
            "nativeFailure":{"operation":"thread/read","stage":"inspect","cause":"invalidThreadResponse","generation":generation,"resume":"notRequested","submission":"notDispatched"}},
            "reachability":"codexAppServer","client":null}),
    ];
    {
        let connection = Connection::open(&source)?;
        for (index, receipt) in receipts.iter().enumerate() {
            connection.execute("UPDATE router_pushes SET delivery_state=?,last_outcome_json=?,settled_at='2026-10-21T12:35:00Z' WHERE push_id=?", params![if index==0 {"delivered"} else {"rejected"}, serde_json::to_string_pretty(receipt)?, ids[index]])?;
        }
    }
    let original = std::fs::read(&source)?;
    let query = "SELECT last_outcome_json AS value FROM router_pushes ORDER BY push_id";
    let encoded = text_rows(&source, query)?;
    let upgrade = MessageUpgrade::prepare(&source, &Default::default()).await?;
    upgrade.copy_to(&destination).await?;
    assert_eq!(text_rows(&destination, query)?, encoded);
    let owner = InteractionStore::open(&destination).await?;
    for (id, expected) in ids.iter().zip(&receipts) {
        let record = owner
            .get_push_record(&id.clone().try_into()?)
            .await?
            .ok_or("released receipt missing")?;
        assert_eq!(
            serde_json::to_value(record.last_outcome.ok_or("receipt missing")?)?,
            *expected
        );
    }
    owner.close().await?;
    upgrade.copy_to(&destination).await?;
    assert_eq!(text_rows(&destination, query)?, encoded);
    assert_eq!(std::fs::read(&source)?, original);
    upgrade.apply_drop(&destination).await?;
    assert!(source_tables(&source)?.is_empty());
    assert_eq!(text_rows(&destination, query)?, encoded);
    Ok(())
}

#[tokio::test]
async fn changed_source_refuses_drop_after_destination_commit() -> TestResult {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("automation.sqlite");
    let destination = directory.path().join("interaction.sqlite");
    seeded_source(&source).await?;
    let prepared = MessageUpgrade::prepare(&source, &Default::default()).await?;
    prepared.copy_to(&destination).await?;
    {
        let connection = Connection::open(&source)?;
        connection.execute("UPDATE router_pushes SET body='new source contents'", [])?;
    }
    let changed = std::fs::read(&source)?;
    assert!(prepared.apply_drop(&destination).await.is_err());
    assert_eq!(std::fs::read(&source)?, changed);
    assert_eq!(source_tables(&source)?.len(), 5);
    Ok(())
}

#[tokio::test]
async fn invalid_destination_additions_or_changed_worker_payload_never_authorize_drop() -> TestResult
{
    for defect in ["worker-id", "retention", "worker-payload"] {
        let directory = tempfile::tempdir()?;
        let source = directory.path().join("automation.sqlite");
        let destination = directory.path().join("interaction.sqlite");
        seeded_source(&source).await?;
        let prepared = MessageUpgrade::prepare(&source, &Default::default()).await?;
        prepared.copy_to(&destination).await?;
        {
            let connection = Connection::open(&destination)?;
            match defect {
                "worker-id" => {
                    connection.execute("UPDATE subscription_delivery_progress SET progress_id='550e8400-e29b-41d4-a716-446655440000'", [])?;
                }
                "retention" => {
                    connection
                        .execute("UPDATE router_pushes SET retention='mailTerminalAge'", [])?;
                }
                "worker-payload" => {
                    connection.execute("UPDATE subscription_delivery_progress SET payload=json_set(payload,'$.generation',2)", [])?;
                }
                _ => return Err("unexpected destination fixture defect".into()),
            }
        }
        let source_before = std::fs::read(&source)?;
        assert!(
            prepared.apply_drop(&destination).await.is_err(),
            "{defect} authorized the drop"
        );
        assert_eq!(std::fs::read(&source)?, source_before);
        assert_eq!(source_tables(&source)?.len(), 5);
    }
    Ok(())
}

#[tokio::test]
async fn an_absent_source_is_explicit_and_never_creates_a_message_store() -> TestResult {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("automation.sqlite");
    let destination = directory.path().join("interaction.sqlite");
    let prepared = MessageUpgrade::prepare(&source, &Default::default()).await?;
    assert_eq!(
        prepared.copy_to(&destination).await?,
        MessageUpgradeReport::NoSource
    );
    assert_eq!(
        prepared.apply_drop(&destination).await?,
        MessageUpgradeReport::NoSource
    );
    assert!(!source.exists() && !destination.exists());
    Ok(())
}

#[tokio::test]
async fn incomplete_or_foreign_source_profiles_refuse_without_mutation() -> TestResult {
    for defect in [
        "missing-table",
        "checksum",
        "future",
        "unknown-table",
        "invalid-state",
        "invalid-retention",
        "bad-natural-key",
        "nonempty-mail",
    ] {
        let directory = tempfile::tempdir()?;
        let source = directory.path().join("automation.sqlite");
        seeded_source(&source).await?;
        {
            let connection = Connection::open(&source)?;
            match defect {
                "missing-table" => {
                    connection.execute("DROP TABLE subscription_delivery_progress", [])?;
                }
                "checksum" => {
                    connection.execute("UPDATE _sqlx_migrations SET checksum=x'00' WHERE version=(SELECT max(version) FROM _sqlx_migrations)",[])?;
                }
                "future" => {
                    connection.execute("INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time) VALUES (99999999999999,'future',1,x'00',0)",[])?;
                }
                "unknown-table" => {
                    connection.execute("CREATE TABLE unowned_state(value TEXT)", [])?;
                }
                "invalid-state" => {
                    connection.execute(
                        "UPDATE router_pushes SET delivery_state='unknown-state'",
                        [],
                    )?;
                }
                "invalid-retention" => {
                    connection
                        .execute("UPDATE router_pushes SET retention='unknown-retention'", [])?;
                }
                "bad-natural-key" => {
                    connection.execute(
                        "UPDATE subscription_delivery_progress SET membership_id=?",
                        params![uuid::Uuid::now_v7().to_string()],
                    )?;
                }
                "nonempty-mail" => {
                    connection.execute("INSERT INTO mail_items(item_id,digest,direction,status_json,status_version,payload_bytes,created_at) VALUES (?,'digest','local','{}',1,0,'2026-10-21T00:00:00Z')",params![uuid::Uuid::now_v7().to_string()])?;
                }
                _ => return Err("unexpected fixture defect".into()),
            }
        }
        let before = std::fs::read(&source)?;
        assert!(
            MessageUpgrade::prepare(&source, &Default::default())
                .await
                .is_err(),
            "{defect} was admitted"
        );
        assert_eq!(std::fs::read(&source)?, before, "{defect} changed source");
    }
    Ok(())
}

#[tokio::test]
async fn known_empty_prefixes_are_supported_but_unversioned_nonempty_intents_refuse() -> TestResult
{
    for version in [
        20_260_930_000_000,
        20_261_012_000_100,
        20_261_013_000_100,
        20_261_015_000_200,
    ] {
        let directory = tempfile::tempdir()?;
        let source_path = directory.path().join("automation.sqlite");
        let destination = directory.path().join("interaction.sqlite");
        source(&source_path, version).await?;
        MessageUpgrade::prepare(&source_path, &Default::default())
            .await?
            .copy_to(&destination)
            .await?;
        if version == 20_261_013_000_100 {
            let connection = Connection::open(&source_path)?;
            connection.execute(
                "INSERT INTO board_intent_progress(project_id,intent_id) VALUES (?,?)",
                params![
                    uuid::Uuid::now_v7().to_string(),
                    uuid::Uuid::now_v7().to_string()
                ],
            )?;
            drop(connection);
            assert!(
                MessageUpgrade::prepare(&source_path, &Default::default())
                    .await
                    .is_err()
            );
        }
    }
    Ok(())
}
