//! Real Router-local key reopen and hard rejection of the retired token format.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use message_board_storage::{
    CursorSigningKey, CursorStore, RouterLocalStorageError, decode_board_cursor,
    encode_board_cursor,
};
use sha2::Sha256;
use sqlx::Connection;
use sqlx_turso::{TursoConnectOptions, TursoConnection};
use std::path::Path;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

async fn cursor_connection(path: &Path) -> TestResult<TursoConnection> {
    Ok(TursoConnection::connect_with(&TursoConnectOptions::new().filename(path)).await?)
}

async fn stored_key_identity(path: &Path) -> TestResult<String> {
    let mut connection = cursor_connection(path).await?;
    let identity = sqlx::query_scalar("SELECT key_id FROM cursor_signing_key")
        .fetch_one(&mut connection)
        .await?;
    connection.close().await?;
    Ok(identity)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn signing_key_uses_a_uuid_v7_identity_that_survives_reopen() -> TestResult {
    // Arrange an empty native store; installation owns both the key and its identity.
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("cursors.sqlite");
    let key = CursorSigningKey::try_from(vec![9; 32])?;
    let store = CursorStore::open(&path).await?;
    assert_eq!(store.key().await?, None);
    assert_eq!(store.get_or_install_key(&key).await?, key);
    let identity = stored_key_identity(&path).await?;
    let parsed = uuid::Uuid::parse_str(&identity)?;
    assert_eq!(parsed.get_version_num(), 7);
    assert_eq!(parsed.get_variant(), uuid::Variant::RFC4122);
    assert_eq!(parsed.hyphenated().to_string(), identity);
    store.close().await;

    // Act through a fresh owning store with a different candidate.
    let reopened = CursorStore::open(&path).await?;
    let replacement = CursorSigningKey::try_from(vec![8; 32])?;
    let installed = reopened.get_or_install_key(&replacement).await?;
    let read = reopened.key().await?;
    reopened.close().await;

    // Assert both durable facts remain exactly the first installation's values.
    assert_eq!(installed, key);
    assert_eq!(read, Some(key));
    assert_eq!(stored_key_identity(&path).await?, identity);
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_second_valid_signing_key_row_is_refused_without_replacement() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("cursors.sqlite");
    let store = CursorStore::open(&path).await?;
    let key = CursorSigningKey::try_from(vec![9; 32])?;
    store.get_or_install_key(&key).await?;
    store.close().await;
    let mut connection = cursor_connection(&path).await?;
    sqlx::query("INSERT INTO cursor_signing_key(key_id,key_bytes) VALUES(?,?)")
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(vec![8_u8; 32])
        .execute(&mut connection)
        .await?;

    let reopened = CursorStore::open(&path).await?;
    let read = reopened.key().await;
    let install = reopened.get_or_install_key(&key).await;
    reopened.close().await;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cursor_signing_key")
        .fetch_one(&mut connection)
        .await?;
    connection.close().await?;
    assert!(
        matches!(read, Err(RouterLocalStorageError::InvalidRecord)),
        "multiple signing-key rows were accepted by key()"
    );
    assert!(
        matches!(install, Err(RouterLocalStorageError::InvalidRecord)),
        "multiple signing-key rows were accepted by get_or_install_key()"
    );
    assert_eq!(count, 2, "invalid stored state must not be rewritten");
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn malformed_signing_key_identities_are_refused_without_replacement() -> TestResult {
    for invalid in [
        Some("00000000-0000-4000-8000-000000000001"),
        Some("not-a-uuid"),
        Some("0192A1B2-C3D4-7000-8000-000000000001"),
        None,
    ] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("cursors.sqlite");
        let store = CursorStore::open(&path).await?;
        let key = CursorSigningKey::try_from(vec![9; 32])?;
        store.get_or_install_key(&key).await?;
        store.close().await;
        let mut connection = cursor_connection(&path).await?;
        sqlx::query("UPDATE cursor_signing_key SET key_id=?")
            .bind(invalid)
            .execute(&mut connection)
            .await?;

        let reopened = CursorStore::open(&path).await?;
        let read = reopened.key().await;
        let install = reopened.get_or_install_key(&key).await;
        reopened.close().await;
        let stored: Option<String> = sqlx::query_scalar("SELECT key_id FROM cursor_signing_key")
            .fetch_one(&mut connection)
            .await?;
        connection.close().await?;
        assert!(
            matches!(
                read,
                Err(RouterLocalStorageError::InvalidRecordKey {
                    field: "cursor_signing_key.key_id"
                })
            ),
            "key() accepted invalid identity {invalid:?}"
        );
        assert!(
            matches!(
                install,
                Err(RouterLocalStorageError::InvalidRecordKey {
                    field: "cursor_signing_key.key_id"
                })
            ),
            "get_or_install_key() accepted invalid identity {invalid:?}"
        );
        assert_eq!(stored.as_deref(), invalid, "invalid identity was replaced");
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_malformed_signing_key_is_refused_without_replacement() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("cursors.sqlite");
    let store = CursorStore::open(&path).await?;
    let key = CursorSigningKey::try_from(vec![9; 32])?;
    store.get_or_install_key(&key).await?;
    store.close().await;
    let mut connection = cursor_connection(&path).await?;
    let invalid = vec![6_u8; 31];
    sqlx::query("UPDATE cursor_signing_key SET key_bytes=?")
        .bind(&invalid)
        .execute(&mut connection)
        .await?;

    let reopened = CursorStore::open(&path).await?;
    let read = reopened.key().await;
    let install = reopened.get_or_install_key(&key).await;
    reopened.close().await;
    let stored: Vec<u8> = sqlx::query_scalar("SELECT key_bytes FROM cursor_signing_key")
        .fetch_one(&mut connection)
        .await?;
    connection.close().await?;
    assert!(matches!(read, Err(RouterLocalStorageError::InvalidCursor)));
    assert!(matches!(
        install,
        Err(RouterLocalStorageError::InvalidCursor)
    ));
    assert_eq!(stored, invalid, "invalid key was replaced");
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn key_reopen_accepts_current_tokens_and_refuses_validly_signed_old_format() -> TestResult {
    // Arrange the real native store, then reopen with a different candidate key.
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("cursors.sqlite");
    let key = CursorSigningKey::try_from(vec![9; 32])?;
    let store = CursorStore::open(&path).await?;
    assert_eq!(store.get_or_install_key(&key).await?, key);
    store.close().await;
    let reopened = CursorStore::open(&path).await?;
    assert_eq!(
        reopened
            .get_or_install_key(&CursorSigningKey::try_from(vec![8; 32])?)
            .await?,
        key
    );
    let payload = serde_json::json!({"operation":"post_list","through":{"epoch":1,"position":42}});
    let current = encode_board_cursor(&key, &payload)?;

    // Act: construct the retired, correctly signed wire shape using the same key and payload.
    let old_payload = serde_json::to_vec(&payload)?;
    let mut old_signer = Hmac::<Sha256>::new_from_slice(key.as_bytes())?;
    old_signer.update(&old_payload);
    let old = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(old_payload),
        URL_SAFE_NO_PAD.encode(old_signer.finalize().into_bytes())
    );
    let decoded: serde_json::Value = decode_board_cursor(&key, &current)?;

    // Assert explicit cursorInvalid, including a forged current prefix over the old MAC domain.
    assert_eq!(decoded, payload);
    assert!(current.starts_with("board2."));
    for invalid in [
        old.clone(),
        format!("board2.{old}"),
        current.replacen("board2.", "board3.", 1),
        format!("{current}x"),
    ] {
        assert!(matches!(
            decode_board_cursor::<serde_json::Value>(&key, &invalid),
            Err(RouterLocalStorageError::InvalidCursor)
        ));
    }
    reopened.close().await;
    Ok(())
}
