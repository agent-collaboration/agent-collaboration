//! A process can disappear after copy commit without authorizing the source drop.
#![allow(clippy::panic_in_result_fn)]
mod message_support;
mod support;
use board_legacy_upgrade::MessageUpgrade;
use message_support::*;

#[tokio::test]
async fn process_exit_after_copy_retries_the_same_ids_before_real_drop() -> TestResult {
    let directory = tempfile::Builder::new()
        .prefix("message-copy-crash-")
        .tempdir()?;
    let source = directory.path().join("automation.sqlite");
    let destination = directory.path().join("interaction.sqlite");
    seeded_source(&source).await?;
    activity_push(&source, RANGE_ROOT, 3, 9)?;
    expiry_push(
        &source,
        serde_json::json!({"kind":"thread","rootMessageId":RANGE_ROOT}),
    )?;
    expiry_push(
        &source,
        serde_json::json!({"kind":"topic","topicId":RANGE_PROJECT}),
    )?;
    let board = directory.path().join("board.sqlite");
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/legacy-complete.sqlite"),
        &board,
    )?;
    let original = std::fs::read(&source)?;
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "message_copy_checkpoint_child",
                "--ignored",
                "--nocapture",
            ])
            .env("MESSAGE_COPY_CHILD_ROOT", directory.path())
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    assert_eq!(
        output.status.code(),
        Some(93),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read(&source)?, original);
    assert_eq!(source_tables(&source)?.len(), 5);
    let first_ids = text_rows(&destination, IDS)?;
    let first_ranges = text_rows(&destination, RANGES)?;
    let first_origins = text_rows(&destination, ORIGINS)?;
    assert_eq!(first_origins.len(), 3);
    assert_eq!(first_ranges.len(), 1);
    assert_eq!(first_ids.len(), 2);
    let prepared = support::prepare_path(&board).await?;
    let retry = MessageUpgrade::prepare(&source, &prepared.message_activity).await?;
    retry.copy_to(&destination).await?;
    assert_eq!(text_rows(&destination, IDS)?, first_ids);
    assert_eq!(text_rows(&destination, RANGES)?, first_ranges);
    assert_eq!(text_rows(&destination, ORIGINS)?, first_origins);
    retry.apply_drop(&destination).await?;
    assert!(source_tables(&source)?.is_empty());
    assert_eq!(text_rows(&destination, IDS)?, first_ids);
    assert_eq!(text_rows(&destination, RANGES)?, first_ranges);
    assert_eq!(text_rows(&destination, ORIGINS)?, first_origins);
    prepared.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "owned subprocess exits after the real destination copy commit"]
async fn message_copy_checkpoint_child() -> TestResult {
    let root = std::path::PathBuf::from(
        std::env::var_os("MESSAGE_COPY_CHILD_ROOT").ok_or("missing child root")?,
    );
    if root.parent() != Some(std::env::temp_dir().as_path())
        || !root
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("message-copy-crash-"))
    {
        return Err("refusing a non-fixture message copy root".into());
    }
    let prepared = support::prepare_path(&root.join("board.sqlite")).await?;
    MessageUpgrade::prepare(&root.join("automation.sqlite"), &prepared.message_activity)
        .await?
        .copy_to(&root.join("interaction.sqlite"))
        .await?;
    std::process::exit(93);
}
