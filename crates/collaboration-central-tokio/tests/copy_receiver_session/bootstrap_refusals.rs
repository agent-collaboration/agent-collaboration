//! Bootstrap rollback certainty crosses the real Copy/session boundary unchanged.

use super::*;
use collaboration_central_conformance::{RuntimeControl, RuntimeControlRequest, owner_assertion};
use collaboration_central_core::ObjectAddress;
use std::{num::NonZeroU64, time::Duration};

fn schema_piece(database: DatabaseId, sequence: u64) -> ConformanceResult<Piece<BoardAnnotation>> {
    let epoch = Epoch::new(1)?;
    Ok(Piece::new(PieceContents {
        database,
        epoch,
        continues_from: ReplicationPosition::new(epoch, CommitSequence::new(sequence - 1)),
        through: ReplicationPosition::new(epoch, CommitSequence::new(sequence)),
        transactions: vec![ReplicatedTransaction::SchemaStep {
            commit_id: CommitSequence::new(sequence),
            migration_version: sequence,
            statements: vec![if sequence == 1 {
                "CREATE TABLE control_rows(record_key TEXT NOT NULL PRIMARY KEY)".into()
            } else {
                "ALTER TABLE control_rows ADD COLUMN label TEXT".into()
            }],
        }],
        part: None,
        annotation: None,
    })?)
}
async fn control(
    base: &str,
    address: ObjectAddress,
    command: RuntimeControl,
) -> ConformanceResult<()> {
    reqwest::Client::new()
        .post(format!("{base}/admin/v1/conformance/runtime"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&RuntimeControlRequest { address, command })
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn bootstrap_full_preserves_correlated_no_effect_while_an_unrelated_copy_progresses_and_reply_loss_stays_unknown()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start_conformance(TokioSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: "019f0000-0000-7000-8000-000000005240"
                .to_owned()
                .try_into()?,
        },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let client = ConformanceClient::new(server.base_url())?;
    let service =
        message_board::ServiceId::try_from("550e8400-e29b-41d4-a716-446655441045".to_owned())?;
    let mut holder = enroll_admitted_peer(
        &client,
        service.as_str(),
        "019f0000-0000-7000-8000-000000001045",
        "bootstrap-full-writer",
        "019f0000-0000-7000-8000-000000011045",
    )
    .await?;
    let healthy = DatabaseId::new(durable_steps::Uuid::now_v7())?;
    let full = DatabaseId::new(durable_steps::Uuid::now_v7())?;
    let proof=async {
        let copies=copy_wait_scenarios::CopyClient::new(server.base_url())?;
        let created=copies.invoke(json!({"kind":"create","value":{"database":healthy,"epoch":1,"holder":holder.peer,"origin":{"kind":"empty"}}})).await?;
        copies.completed(created,copy_wait_scenarios::expected("created",CreateCopyAnswer::CopyCreated)).await?;
        assert_eq!(request(&mut holder.socket,ReplicationRequest::Piece(schema_piece(healthy,1)?)).await?,ReplicationAnswer::Applied {through:ReplicationPosition::new(Epoch::new(1)?,CommitSequence::new(1))});
        // Health opens the empty owning store without invoking bootstrap.
        let before:serde_json::Value=reqwest::Client::new().get(format!("{}/admin/v1/copies/{full}/health",server.base_url()))
            .header("Cf-Access-Jwt-Assertion",owner_assertion()?).send().await?.error_for_status()?.json().await?;
        for offered in [ReplicationRequest::CopyStatus {database:full},ReplicationRequest::SnapshotPage {database:full,cursor:None},ReplicationRequest::Piece(schema_piece(full,1)?)] {
            control(server.base_url(),ObjectAddress::Copy(full),RuntimeControl::CopyFull {after_writes:NonZeroU64::new(1).ok_or("invalid threshold")?}).await?;
            assert_eq!(request(&mut holder.socket,offered).await?,ReplicationAnswer::CopyUnavailable {reason:CopyUnavailableReason::StorageFull});
        }
        let after:serde_json::Value=reqwest::Client::new().get(format!("{}/admin/v1/copies/{full}/health",server.base_url()))
            .header("Cf-Access-Jwt-Assertion",owner_assertion()?).send().await?.error_for_status()?.json().await?;
        for field in ["bookkeeping","stage","copy","projectFingerprint","stagedParts"] {
            let pointer=format!("/value/{field}");
            let original=before.pointer(&pointer).ok_or_else(||format!("Health omitted {field}"))?;
            assert_eq!(after.pointer(&pointer),Some(original),"bootstrap rollback changed {field}");
        }
        assert_eq!(request(&mut holder.socket,ReplicationRequest::Piece(schema_piece(healthy,2)?)).await?,ReplicationAnswer::Applied {through:ReplicationPosition::new(Epoch::new(1)?,CommitSequence::new(2))});
        control(server.base_url(),ObjectAddress::Connection(service),RuntimeControl::DropNextObjectReply {destination:ObjectAddress::Copy(healthy)}).await?;
        assert!(matches!(request(&mut holder.socket,ReplicationRequest::CopyStatus {database:healthy}).await?,ReplicationAnswer::OutcomeUnknown {..}));
        assert!(matches!(request(&mut holder.socket,ReplicationRequest::CopyStatus {database:healthy}).await?,ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status)) if status.acked.sequence.value()==2));
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let _ = holder
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    proof?;
    // Independent read of only the consumer's offered schema after shutdown.
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.path().join(format!("copy-{full}.db")))
        .read_only(true)
        .connect()
        .await?;
    let tables = query::<sqlx_turso::Turso>(
        "SELECT count(*) AS count FROM sqlite_schema WHERE name='control_rows'",
    )
    .fetch_one(&mut connection)
    .await?
    .try_get::<i64, _>("count")?;
    sqlx_core::connection::Connection::close(connection).await?;
    assert_eq!(
        tables, 0,
        "rolled-back bootstrap allowed the user Piece to run"
    );
    Ok(())
}
