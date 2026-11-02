//! Real CLI processes reach the enrolled Host's central instruction catalog.
#[path = "support/central_automation_host.rs"]
#[expect(
    dead_code,
    reason = "This target uses instruction CRUD, not the fixture's controlled firing journey."
)]
mod fixture;
use fixture::{CentralHost, ProofResult};
use serde_json::Value;

#[tokio::test]
async fn cli_creates_and_reads_instruction_through_host() -> ProofResult {
    let host = CentralHost::start(fixture::directory()?).await?;
    let proof: ProofResult = async {
        let created = host
            .json(&["instruction", "create", "--text", "Check repository"])
            .await?;
        let id = created
            .pointer("/result/record/instructionId")
            .and_then(Value::as_str)
            .ok_or("missing created instruction identity")?;
        let read = host
            .json(&["instruction", "show", "--instruction-id", id])
            .await?;
        if read.pointer("/result/record/text").and_then(Value::as_str) != Some("Check repository") {
            return Err("CLI text was not persisted".into());
        }
        use sqlx::Connection as _;
        let mut sql = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(host.root().join("automation.sqlite")),
        )
        .await?;
        let local: i64 =
            sqlx::query_scalar("SELECT count(*) FROM instruction_documents WHERE instruction_id=?")
                .bind(id)
                .fetch_one(&mut sql)
                .await?;
        sql.close().await?;
        if local != 0 {
            return Err("new CLI Instruction created local definition truth".into());
        }
        Ok(())
    }
    .await;
    let stopped = host.close().await;
    proof?;
    stopped
}
