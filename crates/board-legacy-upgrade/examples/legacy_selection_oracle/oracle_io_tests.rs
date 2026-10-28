use crate::{
    execute_legacy_selection_oracle, oracle_arguments::OracleArguments, oracle_errors::OracleError,
    oracle_receipt::validate_owner_private_output_path, read_sealed_copy,
};
use clap::Parser;
use std::path::Path;

fn create_empty_legacy_source_database(
    source_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let connection = rusqlite::Connection::open(source_path)?;
    create_legacy_source_tables(&connection)?;
    Ok(())
}

fn create_legacy_source_tables(
    connection: &rusqlite::Connection,
) -> Result<(), Box<dyn std::error::Error>> {
    connection.execute_batch(
        "CREATE TABLE board_identities(identity_key TEXT);
         CREATE TABLE board_topics(topic_id TEXT);
         CREATE TABLE board_activity(activity_sequence INTEGER, project_id TEXT,
           topic_id TEXT, root_id TEXT, kind TEXT, actor_key TEXT, message_id TEXT);
         CREATE TABLE project_reader_state(reader_key TEXT, project_id TEXT, main_start INTEGER);
         CREATE TABLE topic_read_bookmarks(reader_key TEXT, topic_id TEXT, through_activity INTEGER);
         CREATE TABLE thread_read_bookmarks(reader_key TEXT, root_id TEXT, through_activity INTEGER);
         CREATE TABLE thread_watches(reader_key TEXT, root_id TEXT,
           starts_after_activity INTEGER, active INTEGER);
         CREATE TABLE topic_watches(reader_key TEXT, topic_id TEXT,
           starts_after_activity INTEGER, active INTEGER);",
    )?;
    Ok(())
}

fn create_empty_cli_legacy_source_database(
    source_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let connection = rusqlite::Connection::open(source_path)?;
    create_legacy_source_tables(&connection)?;
    connection.execute_batch(
        "INSERT INTO board_identities VALUES('human:legacy-owner');
         INSERT INTO board_topics VALUES('019a0000-0000-7000-8000-000000000015');
         INSERT INTO topic_watches VALUES(
           'human:legacy-owner', '019a0000-0000-7000-8000-000000000015', 0, 1
         );",
    )?;
    Ok(())
}

fn create_nonempty_cli_legacy_source_database(
    source_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let connection = rusqlite::Connection::open(source_path)?;
    create_legacy_source_tables(&connection)?;
    connection.execute_batch(
        "INSERT INTO board_identities VALUES('human:legacy-owner');
         INSERT INTO board_topics VALUES('019a0000-0000-7000-8000-000000000015');
         INSERT INTO board_activity VALUES
           (1, 'workspace', '019a0000-0000-7000-8000-000000000015',
            NULL, 'mainMessageCreated', 'other', 'root'),
           (2, 'workspace', '019a0000-0000-7000-8000-000000000015',
            'root', 'threadMessageCreated', 'other', 'reply-at-thread-mark'),
           (3, 'workspace', '019a0000-0000-7000-8000-000000000015',
            'root', 'threadMessageCreated', 'other', 'reply-before-watch-start'),
           (4, 'workspace', '019a0000-0000-7000-8000-000000000015',
            'root', 'threadMessageCreated', 'other', 'reply-at-watch-boundary'),
           (5, 'workspace', '019a0000-0000-7000-8000-000000000015',
            'root', 'threadMessageCreated', 'other', 'reply-at-main-start'),
           (6, 'workspace', '019a0000-0000-7000-8000-000000000015',
            NULL, 'mainMessageCreated', 'other', 'main-before-topic-mark'),
           (7, 'workspace', '019a0000-0000-7000-8000-000000000015',
            NULL, 'mainMessageCreated', 'other', 'main-after-topic-mark'),
           (8, 'workspace', '019a0000-0000-7000-8000-000000000015',
            'root', 'threadMessageCreated', 'human:legacy-owner', 'own-reply'),
           (9, 'workspace', '019a0000-0000-7000-8000-000000000015',
            NULL, 'other', 'human:legacy-owner', NULL);
         INSERT INTO project_reader_state
           VALUES('human:legacy-owner', 'workspace', 5);
         INSERT INTO topic_read_bookmarks
           VALUES('human:legacy-owner', '019a0000-0000-7000-8000-000000000015', 6);
         INSERT INTO thread_read_bookmarks
           VALUES('human:legacy-owner', 'root', 2);
         INSERT INTO thread_watches
           VALUES('human:legacy-owner', 'root', 4, 1);",
    )?;
    Ok(())
}

fn private_cache_test_directory(
    prefix: &str,
) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

    let home = std::env::var_os("HOME").ok_or("HOME is required for the private output fixture")?;
    let cache_root = PathBuf::from(home).join(".cache");
    fs::create_dir_all(&cache_root)?;
    let private_directory = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(cache_root)?;
    fs::set_permissions(private_directory.path(), fs::Permissions::from_mode(0o700))?;
    Ok(private_directory)
}

#[test]
fn cli_covers_state_free_reader_and_empty_topic() -> Result<(), Box<dyn std::error::Error>> {
    let private_directory = private_cache_test_directory("legacy-selection-oracle-test-")?;
    let source_path = private_directory.path().join("legacy.sqlite");
    let output_path = private_directory.path().join("oracle.json");
    create_empty_cli_legacy_source_database(&source_path)?;

    let status = execute_legacy_selection_oracle(cli_arguments(
        &source_path,
        private_directory.path(),
        &output_path,
    )?)?;
    assert_empty_cli_status_and_receipt(&status, &output_path)?;
    assert_empty_cli_output_is_create_new_and_source_is_sealed(
        &source_path,
        private_directory.path(),
        &output_path,
    )?;
    Ok(())
}

fn assert_empty_cli_status_and_receipt(
    status: &crate::oracle_receipt::OracleStatus,
    output_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;

    assert_eq!(status.kind, "legacyOracleOnly");
    assert_eq!(status.activity_count, 0);
    assert_eq!(status.selection_count, 2);
    assert_eq!(status.product_assertions, 0);

    let receipt: serde_json::Value = serde_json::from_slice(&fs::read(output_path)?)?;
    assert_eq!(
        receipt.get("kind"),
        Some(&serde_json::json!("legacyOracleOnly"))
    );
    assert_eq!(
        receipt.get("product_import_observed"),
        Some(&serde_json::json!(false))
    );
    assert_eq!(receipt.get("activity_count"), Some(&serde_json::json!(0)));
    assert_eq!(
        receipt.get("selections"),
        Some(&serde_json::json!([
            {
                "reader": "human:legacy-owner",
                "legacy_topic": "019a0000-0000-7000-8000-000000000015",
                "mode": "latest",
                "post_ids": [],
                "post_count": 0,
                "covered_post_set_sha256": "4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945"
            },
            {
                "reader": "human:legacy-owner",
                "legacy_topic": "019a0000-0000-7000-8000-000000000015",
                "mode": "unread",
                "post_ids": [],
                "post_count": 0,
                "covered_post_set_sha256": "4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945"
            }
        ])),
    );
    assert_eq!(
        receipt.get("projects"),
        Some(&serde_json::json!([{
            "legacy_topic": "019a0000-0000-7000-8000-000000000015",
            "message_creation_activity_count": 0,
            "readers": [{
                "reader": "human:legacy-owner",
                "main_starts": [],
                "topic_mark_through_activity": 0,
                "thread_cutoffs": []
            }]
        }])),
    );
    let source_sha256 = receipt
        .get("source_sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or("receipt omitted source hash")?;
    assert!(
        source_sha256.len() == 64
            && source_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    );
    let driver_sha256 = receipt
        .get("driver_sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or("receipt omitted driver hash")?;
    assert!(
        driver_sha256.len() == 64
            && driver_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(output_path)?.permissions().mode() & 0o777,
            0o600
        );
    }
    Ok(())
}

fn assert_empty_cli_output_is_create_new_and_source_is_sealed(
    source_path: &Path,
    copy_root: &Path,
    output_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::{fs, path::PathBuf};

    let retained_receipt = fs::read(output_path)?;
    let error =
        execute_legacy_selection_oracle(cli_arguments(source_path, copy_root, output_path)?)
            .err()
            .ok_or("oracle overwrote its existing output")?;
    assert!(matches!(error, OracleError::OutputAlreadyExists));
    assert_eq!(fs::read(output_path)?, retained_receipt);

    let mut sidecar_path = source_path.as_os_str().to_os_string();
    sidecar_path.push("-wal");
    let sidecar_path = PathBuf::from(sidecar_path);
    fs::write(&sidecar_path, b"sidecar fixture")?;
    assert!(matches!(
        read_sealed_copy(source_path, copy_root),
        Err(OracleError::UnsealedLegacySource)
    ));
    Ok(())
}

#[test]
fn cli_receipt_selects_nonempty_sealed_legacy_history() -> Result<(), Box<dyn std::error::Error>> {
    let private_directory = private_cache_test_directory("legacy-selection-oracle-nonempty-test-")?;
    let source_path = private_directory.path().join("legacy.sqlite");
    let output_path = private_directory.path().join("oracle.json");
    create_nonempty_cli_legacy_source_database(&source_path)?;

    let status = execute_legacy_selection_oracle(cli_arguments(
        &source_path,
        private_directory.path(),
        &output_path,
    )?)?;

    assert_eq!(status.kind, "legacyOracleOnly");
    assert_eq!(status.activity_count, 9);
    assert_eq!(status.selection_count, 2);
    assert_eq!(status.product_assertions, 0);
    assert_nonempty_cli_receipt(&output_path)?;
    Ok(())
}

fn assert_nonempty_cli_receipt(output_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;

    let receipt: serde_json::Value = serde_json::from_slice(&fs::read(output_path)?)?;
    assert_eq!(
        receipt.get("kind"),
        Some(&serde_json::json!("legacyOracleOnly"))
    );
    assert_eq!(
        receipt.get("product_import_observed"),
        Some(&serde_json::json!(false))
    );
    assert_eq!(receipt.get("activity_count"), Some(&serde_json::json!(9)));
    let selections = receipt
        .get("selections")
        .and_then(serde_json::Value::as_array)
        .ok_or("receipt omitted selections")?;
    assert_eq!(selections.len(), 2);
    assert_eq!(
        selections[0].get("mode"),
        Some(&serde_json::json!("latest"))
    );
    assert_eq!(
        selections[0].get("post_ids"),
        Some(&serde_json::json!([
            "own-reply",
            "main-after-topic-mark",
            "main-before-topic-mark",
            "reply-at-main-start",
            "reply-at-watch-boundary",
            "reply-before-watch-start",
            "reply-at-thread-mark",
            "root"
        ]))
    );
    assert_eq!(selections[0].get("post_count"), Some(&serde_json::json!(8)));
    assert_eq!(
        selections[1].get("mode"),
        Some(&serde_json::json!("unread"))
    );
    assert_eq!(
        selections[1].get("post_ids"),
        Some(&serde_json::json!([
            "reply-at-main-start",
            "main-after-topic-mark"
        ]))
    );
    assert_eq!(selections[1].get("post_count"), Some(&serde_json::json!(2)));
    assert_eq!(
        receipt.get("projects"),
        Some(&serde_json::json!([{
            "legacy_topic": "019a0000-0000-7000-8000-000000000015",
            "message_creation_activity_count": 8,
            "readers": [{
                "reader": "human:legacy-owner",
                "main_starts": [{
                    "legacy_workspace": "workspace",
                    "main_start": 5
                }],
                "topic_mark_through_activity": 6,
                "thread_cutoffs": [{
                    "root_id": "root",
                    "watch_active": true,
                    "watch_starts_after_activity": 4,
                    "thread_mark_through_activity": 2
                }]
            }]
        }])),
    );
    Ok(())
}

#[test]
fn sealed_reader_rejects_outside_roots_and_normal_state_paths()
-> Result<(), Box<dyn std::error::Error>> {
    use std::{fs, os::unix::fs::PermissionsExt};

    let home = std::env::var_os("HOME").ok_or("HOME is required for the private source fixture")?;
    let cache_root = std::path::PathBuf::from(home).join(".cache");
    fs::create_dir_all(&cache_root)?;
    let private_directory = tempfile::Builder::new()
        .prefix("legacy-selection-source-test-")
        .tempdir_in(cache_root)?;
    fs::set_permissions(private_directory.path(), fs::Permissions::from_mode(0o700))?;
    let source_path = private_directory.path().join("legacy.sqlite");
    let connection = rusqlite::Connection::open(&source_path)?;
    drop(connection);

    let unrelated_root = private_directory.path().join("other-copy-root");
    fs::create_dir(&unrelated_root)?;
    assert!(matches!(
        read_sealed_copy(&source_path, &unrelated_root),
        Err(OracleError::InvalidLegacySourcePath)
    ));

    for directory_name in [".codex", ".codex-router", ".codex-router-debug"] {
        let normal_state_root = private_directory.path().join(directory_name);
        fs::create_dir(&normal_state_root)?;
        let normal_state_copy = normal_state_root.join("legacy.sqlite");
        fs::write(&normal_state_copy, b"not opened by the oracle")?;
        assert!(matches!(
            read_sealed_copy(&normal_state_copy, private_directory.path()),
            Err(OracleError::NormalStateSourcePath)
        ));
    }
    Ok(())
}

#[test]
fn sealed_reader_checks_ancestor_names_not_the_sqlite_file_name()
-> Result<(), Box<dyn std::error::Error>> {
    let source_root = tempfile::tempdir()?;
    let source_path = source_root.path().join(".codex-router");
    create_empty_legacy_source_database(&source_path)?;

    let sealed_copy = read_sealed_copy(&source_path, source_root.path())?;

    assert!(sealed_copy.rows.activity.is_empty());
    Ok(())
}

#[test]
fn output_directory_must_be_owner_private_under_cache() -> Result<(), Box<dyn std::error::Error>> {
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

    let outside_cache = tempfile::tempdir()?;
    let output_path = outside_cache.path().join("oracle.json");

    assert!(matches!(
        validate_owner_private_output_path(&output_path),
        Err(OracleError::InvalidOutputDirectory)
    ));

    let home = std::env::var_os("HOME").ok_or("HOME is required for the private output fixture")?;
    let cache_root = PathBuf::from(home).join(".cache");
    fs::create_dir_all(&cache_root)?;
    let shared_directory = tempfile::Builder::new()
        .prefix("legacy-selection-shared-output-test-")
        .tempdir_in(cache_root)?;
    fs::set_permissions(shared_directory.path(), fs::Permissions::from_mode(0o755))?;
    let shared_output_path = shared_directory.path().join("oracle.json");
    assert!(matches!(
        validate_owner_private_output_path(&shared_output_path),
        Err(OracleError::InvalidOutputDirectory)
    ));
    Ok(())
}

fn cli_arguments(
    source_path: &Path,
    copy_root: &Path,
    output_path: &Path,
) -> Result<OracleArguments, Box<dyn std::error::Error>> {
    let source_path = source_path.to_str().ok_or("source path is not UTF-8")?;
    let copy_root = copy_root.to_str().ok_or("copy root is not UTF-8")?;
    let output_path = output_path.to_str().ok_or("output path is not UTF-8")?;
    Ok(OracleArguments::try_parse_from([
        "legacy_selection_oracle",
        "--legacy-copy",
        source_path,
        "--copy-root",
        copy_root,
        "--output",
        output_path,
    ])?)
}
