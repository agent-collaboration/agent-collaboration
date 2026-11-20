use super::*;
use central_storage::SqlValue;
use sqlx::{Connection, Row, SqliteConnection};

fn claim() -> ReceiptClaim {
    ReceiptClaim {
        request: "01890f2e-7b4c-7cc0-98c4-000000000001".parse().unwrap(),
        caller: "01890f2e-7b4c-7cc0-98c4-000000000002".parse().unwrap(),
        digest: "content".into(),
        started_at: EngineTime::from_timestamp_millis(86_400_001).unwrap(),
        cutoff: EngineTime::from_timestamp_millis(1).unwrap(),
    }
}
#[tokio::test]
async fn a_live_receipt_cannot_be_overwritten_and_expiry_is_one_millisecond_exclusive() {
    let mut connection = SqliteConnection::connect("sqlite::memory:").await.unwrap();
    fleet_storage_schema::workspace_migrations::central_migrator()
        .run(&mut connection)
        .await
        .unwrap();
    let claim = claim();
    sqlx::query("INSERT INTO runtime_request_log VALUES (?,'original caller','original digest','original answer',?,?)")
        .bind(claim.request.as_str()).bind(timestamp(claim.cutoff)).bind(timestamp(claim.cutoff))
        .execute(&mut connection).await.unwrap();
    let answer = EngineReceipt::<String, String>::Applied {
        state: "new".into(),
    };
    for expired in [false, true] {
        if expired {
            sqlx::query("UPDATE runtime_request_log SET created_at=?")
                .bind(timestamp(claim.cutoff - chrono::Duration::milliseconds(1)))
                .execute(&mut connection)
                .await
                .unwrap();
        }
        let mut transaction = connection.begin().await.unwrap();
        let mut failed = false;
        for statement in claim.statements(&answer).unwrap() {
            let mut query = sqlx::query(statement.statement.sql);
            for value in statement.statement.args {
                query = match value {
                    SqlValue::Text(value) => query.bind(value),
                    _ => panic!("receipt uses text binds"),
                };
            }
            match query.execute(&mut *transaction).await {
                Ok(result) => assert!(statement.expect.admits(result.rows_affected())),
                Err(error) => {
                    assert!(!expired);
                    assert!(error.as_database_error().unwrap().is_unique_violation());
                    failed = true;
                    break;
                }
            }
        }
        if failed {
            transaction.rollback().await.unwrap();
        } else {
            transaction.commit().await.unwrap();
        }
        let row =
            sqlx::query("SELECT caller,content_digest,answer,created_at FROM runtime_request_log")
                .fetch_one(&mut connection)
                .await
                .unwrap();
        if expired {
            assert_eq!(row.get::<String, _>("caller"), claim.caller.as_str());
            assert_eq!(
                row.get::<String, _>("answer"),
                serde_json::to_string(&answer).unwrap()
            );
        } else {
            assert_eq!(row.get::<String, _>("caller"), "original caller");
            assert_eq!(row.get::<String, _>("answer"), "original answer");
            assert_eq!(row.get::<String, _>("created_at"), timestamp(claim.cutoff));
        }
    }
}

#[test]
fn replay_checks_caller_and_digest_before_decoding_a_closed_typed_answer() {
    let claim = claim();
    let answer = EngineReceipt::<String, String>::Applied {
        state: "fired".into(),
    };
    let mut rows = ReadOutcome {
        columns: vec!["caller".into(), "content_digest".into(), "answer".into()],
        rows: vec![vec![
            text(&claim.caller),
            text(&claim.digest),
            text(serde_json::to_string(&answer).unwrap()),
        ]],
    };
    assert_eq!(claim.replay::<String, String>(&rows).unwrap(), Some(answer));
    rows.rows[0][0] = text("different");
    rows.rows[0][2] = text("invalid JSON");
    assert_eq!(
        claim.replay::<String, String>(&rows).unwrap(),
        Some(EngineReceipt::RequestIdReused)
    );
    rows.rows[0][0] = text(&claim.caller);
    assert!(claim.replay::<String, String>(&rows).is_err());
    rows.rows[0][2] = text(r#"{"result":"outcomeUnknown"}"#);
    assert!(claim.replay::<String, String>(&rows).is_err());
    assert!(claim.lookup().sql.contains("created_at>=?"));
}
