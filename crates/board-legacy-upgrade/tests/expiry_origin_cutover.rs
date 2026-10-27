//! Decision 1050: the old canonical expiry key carries a retired board scope.
#![expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions fail the test while fixture errors propagate."
)]
mod message_support;
mod support;
use board_legacy_upgrade::{MessageUpgrade, MessageUpgradeError};
use collaboration_protocol::RouterOriginRef;
use interaction_storage::InteractionStore;
use message_board::SubscriptionScope;
use message_support::*;

async fn copied_expiry(
    scope: serde_json::Value,
    expected: SubscriptionScope,
    fixture: &str,
) -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let destination = directory.path().join("interaction.sqlite");
    source(&path, 20_260_930_000_000).await?;
    let id = expiry_push(&path, scope)?;
    let text =
        "SELECT json_object('header',header_facts_json,'body',body) AS value FROM router_pushes";
    let frozen = text_rows(&path, text)?;
    let original = std::fs::read(&path)?;
    let prepared = support::prepare(fixture).await?;
    if let SubscriptionScope::Project { project_id } = &expected {
        assert_eq!(
            prepared
                .report
                .projects
                .iter()
                .find(|report| &report.project == project_id)
                .ok_or("empty project missing")?
                .posts,
            0
        );
    }
    let messages = MessageUpgrade::prepare(&path, &prepared.message_activity).await?;
    messages.copy_to(&destination).await?;
    let owner = InteractionStore::open(&destination).await?;
    let record = owner
        .get_push_record(&id.try_into()?)
        .await?
        .ok_or("expiry missing")?;
    let reference = record.origin_router_ref.ok_or("origin missing")?;
    let RouterOriginRef::SubscriptionExpiry {
        scope,
        subscription_generation,
        target,
    } = RouterOriginRef::parse_canonical(&reference)?
    else {
        return Err("wrong origin variant".into());
    };
    assert_eq!(scope, expected);
    assert_eq!(u64::from(subscription_generation), 1);
    assert_eq!(String::from(target.session_id), "legacy-recipient");
    owner.close().await?;
    assert_eq!(text_rows(&destination, text)?, frozen);
    assert_eq!(std::fs::read(&path)?, original);
    prepared.close().await;
    Ok(())
}

#[tokio::test]
async fn shipped_thread_expiry_origin_reads_current_scope() -> TestResult {
    copied_expiry(
        serde_json::json!({"kind":"thread","rootMessageId":RANGE_ROOT}),
        SubscriptionScope::Thread {
            thread_id: RANGE_ROOT.to_owned().try_into()?,
        },
        "legacy-complete.sqlite",
    )
    .await
}

#[tokio::test]
async fn shipped_topic_expiry_origin_reads_current_scope() -> TestResult {
    copied_expiry(
        serde_json::json!({"kind":"topic","topicId":"019a0000-0000-7000-8000-000000000016"}),
        SubscriptionScope::Project {
            project_id: "019a0000-0000-7000-8000-000000000016"
                .to_owned()
                .try_into()?,
        },
        "role-history.sqlite",
    )
    .await
}

#[tokio::test]
async fn missing_scope_identities_refuse_with_counts_without_creating_a_destination() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    source(&path, 20_260_930_000_000).await?;
    expiry_push(
        &path,
        serde_json::json!({"kind":"thread","rootMessageId":"019a0000-0000-7000-8000-00000000ffff"}),
    )?;
    expiry_push(
        &path,
        serde_json::json!({"kind":"topic","topicId":"019a0000-0000-7000-8000-00000000ffff"}),
    )?;
    let original = std::fs::read(&path)?;
    let prepared = support::prepare("legacy-complete.sqlite").await?;
    assert!(matches!(
        MessageUpgrade::prepare(&path, &prepared.message_activity).await,
        Err(MessageUpgradeError::UnmappedSubscriptionScopes {
            thread_scopes: 1,
            project_scopes: 1
        })
    ));
    assert_eq!(std::fs::read(&path)?, original);
    assert!(!directory.path().join("interaction.sqlite").exists());
    prepared.close().await;
    Ok(())
}

#[tokio::test]
async fn origin_translation_keeps_strict_legacy_shape_and_canonical_identity_admission()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    source(&path, 20_260_930_000_000).await?;
    expiry_push(
        &path,
        serde_json::json!({"kind":"thread","rootMessageId":RANGE_ROOT}),
    )?;
    let original = text_rows(&path, ORIGINS)?
        .pop()
        .ok_or("fixture origin missing")?;
    let prepared = support::prepare("legacy-complete.sqlite").await?;
    for malformed in [
        format!(" {original}"),
        original.replace("rootMessageId", "threadId"),
        original.replace("rootMessageId", "unknownField"),
    ] {
        {
            let connection = rusqlite::Connection::open(&path)?;
            connection.execute("UPDATE router_pushes SET origin_router_ref=?", [malformed])?;
        }
        let before = std::fs::read(&path)?;
        assert!(
            MessageUpgrade::prepare(&path, &prepared.message_activity)
                .await
                .is_err()
        );
        assert_eq!(std::fs::read(&path)?, before);
    }
    prepared.close().await;
    Ok(())
}
