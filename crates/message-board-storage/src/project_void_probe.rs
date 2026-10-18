//! Bounded native diagnostics; toggled enforcement is never production or acceptance proof.
use sqlx::Connection;
use sqlx_turso::{TursoConnectOptions, TursoConnection};

#[tokio::test]
async fn native_large_ledger_parent_update_discriminates_a_read_scan_in_the_write_transaction()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use message_board::*;
    let directory = tempfile::tempdir()?;
    for read_scan in [true, false] {
        {
            let path = directory.path().join(format!("ledger-{read_scan}.sqlite"));
            let mut connection = TursoConnection::connect_with(
                &TursoConnectOptions::new()
                    .filename(&path)
                    .create_if_missing(true),
            )
            .await?;
            crate::PROJECT_MIGRATOR.run(&mut connection).await?;
            sqlx::query("PRAGMA capture_data_changes_conn('full')")
                .execute(&mut connection)
                .await?;
            let project = ProjectId::generate();
            let base = crate::project_store_tests::project_record(project);
            let id = RequestId::generate();
            let mut ledger = RequestLedgerEntry {
                envelope: RecordEnvelope {
                    id: id.clone().into(),
                    request: Some(id.clone()),
                    ..base.envelope().clone()
                },
                request_id: id.clone(),
                canonical_hash: CanonicalHash::try_from("a".repeat(64))?,
                observed_epoch: None,
                accepted_in: Some(EpochNumber::try_from(1)?),
                decision: RequestDecision::Accepted {
                    records: vec![],
                    positions: vec![],
                    change: ChangeOutcome::AlreadySo,
                },
                resubmits: None,
            };
            let payload = serde_json::to_string(&Record::Request(ledger.clone()))?;
            sqlx::query("INSERT INTO record_identities(kind,id,payload) VALUES(?,?,?)")
                .bind("request")
                .bind(id.as_str())
                .bind(&payload)
                .execute(&mut connection)
                .await?;
            let decision = serde_json::to_string(&ledger.decision)?;
            sqlx::query("INSERT INTO request_ledger(request_id,canonical_hash,observed_epoch,accepted_in,decision,resubmits) VALUES(?,?,NULL,1,?,NULL)").bind(id.as_str()).bind(ledger.canonical_hash.as_str()).bind(decision).execute(&mut connection).await?;
            let project_payload = serde_json::to_string(&base)?;
            sqlx::query("INSERT INTO record_identities(kind,id,payload) VALUES(?,?,?)")
                .bind("project")
                .bind(base.envelope().id.as_str())
                .bind(project_payload)
                .execute(&mut connection)
                .await?;
            let other = RequestId::generate();
            let mut other_ledger = ledger.clone();
            other_ledger.request_id = other.clone();
            other_ledger.envelope.id = other.clone().into();
            other_ledger.decision = RequestDecision::Rejected {
                reasons: vec![Refusal::Archived],
            };
            let other_decision = serde_json::to_string(&other_ledger.decision)?;
            let other_payload = serde_json::to_string(&Record::Request(other_ledger))?;
            sqlx::query("INSERT INTO record_identities(kind,id,payload) VALUES(?,?,?)")
                .bind("request")
                .bind(other.as_str())
                .bind(other_payload)
                .execute(&mut connection)
                .await?;
            sqlx::query("INSERT INTO request_ledger(request_id,canonical_hash,observed_epoch,accepted_in,decision,resubmits) VALUES(?,'hash',NULL,1,?,NULL)").bind(other.as_str()).bind(other_decision).execute(&mut connection).await?;
            ledger.decision = RequestDecision::Voided {
                cause: VoidCause::ProjectRejected {
                    reason: Box::new(Refusal::Archived),
                },
            };
            let updated = serde_json::to_string(&Record::Request(ledger))?;
            let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
            if read_scan {
                let _: Vec<(String, String)> = sqlx::query_as(
                    "SELECT id,payload FROM record_identities WHERE kind=? ORDER BY id",
                )
                .bind("request")
                .fetch_all(&mut *transaction)
                .await?;
                let _: String = sqlx::query_scalar(
                    "SELECT payload FROM record_identities WHERE kind=? AND id=?",
                )
                .bind("request")
                .bind(id.as_str())
                .fetch_one(&mut *transaction)
                .await?;
            }
            let outcome =
                sqlx::query("UPDATE record_identities SET payload=? WHERE kind=? AND id=?")
                    .bind(&updated)
                    .bind("request")
                    .bind(id.as_str())
                    .execute(&mut *transaction)
                    .await?;
            assert_eq!(outcome.rows_affected(), 1);
            let stored: String =
                sqlx::query_scalar("SELECT payload FROM record_identities WHERE kind=? AND id=?")
                    .bind("request")
                    .bind(id.as_str())
                    .fetch_one(&mut *transaction)
                    .await?;
            assert_eq!(stored, updated);
            transaction.rollback().await?;
            connection.close().await?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn native_parent_payload_update_discriminates_capture_foreign_keys_and_statement_form()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    for foreign_keys in [true, false] {
        for capture in [true, false] {
            {
                let path = directory
                    .path()
                    .join(format!("probe-{foreign_keys}-{capture}.sqlite"));
                let mut connection = TursoConnection::connect_with(
                    &TursoConnectOptions::new()
                        .filename(&path)
                        .create_if_missing(true)
                        .foreign_keys(foreign_keys),
                )
                .await?;
                crate::PROJECT_MIGRATOR.run(&mut connection).await?;
                if capture {
                    sqlx::query("PRAGMA capture_data_changes_conn('full')")
                        .execute(&mut connection)
                        .await?;
                }
                let id = "01890f2e-7b4c-7cc0-98c4-000000000001";
                // Current metadata indexes require JSON even in this raw SQL diagnostic.
                sqlx::query("INSERT INTO record_identities(kind,id,payload) VALUES(?,?,?)")
                    .bind("request")
                    .bind(id)
                    .bind(r#"{"probe":"old payload"}"#)
                    .execute(&mut connection)
                    .await?;
                let decision = serde_json::to_string(&message_board::RequestDecision::Accepted {
                    records: Vec::new(),
                    positions: Vec::new(),
                    change: message_board::ChangeOutcome::AlreadySo,
                })?;
                sqlx::query("INSERT INTO request_ledger(request_id,canonical_hash,observed_epoch,accepted_in,decision,resubmits) VALUES(?,'hash',NULL,1,?,NULL)").bind(id).bind(decision).execute(&mut connection).await?;
                let outcome=sqlx::query(r#"UPDATE record_identities SET payload='{"probe":"new payload"}' WHERE kind='request' AND id=?"#).bind(id).execute(&mut connection).await?;
                assert_eq!(outcome.rows_affected(), 1);
                let stored: String = sqlx::query_scalar(
                    "SELECT payload FROM record_identities WHERE kind='request' AND id=?",
                )
                .bind(id)
                .fetch_one(&mut connection)
                .await?;
                assert_eq!(stored, r#"{"probe":"new payload"}"#);
                connection.close().await?;
            }
        }
    }
    Ok(())
}
