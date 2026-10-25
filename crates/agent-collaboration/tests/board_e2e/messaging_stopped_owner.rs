//! Prepare private fault input on stopped owners; read custody through the pinned SDK observer.
use super::{native_sql::SqlHost, owned_host::StoppedHost};
use codex_router_test_support::FixtureResult;
use durable_steps::turso_store::TursoScope;
use fleet_sync_delivery::test_support::DeliveryObservation;
use sqlx_turso::sqlx::{ConnectOptions, Connection};
use std::os::unix::fs::OpenOptionsExt as _;

pub use collaboration_service::MailStoreTestFault as MailFault;

pub async fn install_fault(stopped: &StoppedHost, fault: MailFault) -> FixtureResult {
    let path = stopped
        .directory()
        .join(codex_router_host::CollaborationRuntime::RUNTIME_TEST_MAIL_FAULT_FILE);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    serde_json::to_writer(file, &fault)?;
    Ok(())
}

pub async fn remove_faults(stopped: &StoppedHost) -> FixtureResult {
    std::fs::remove_file(
        stopped
            .directory()
            .join(codex_router_host::CollaborationRuntime::RUNTIME_TEST_MAIL_FAULT_FILE),
    )?;
    Ok(())
}

pub async fn delivery_rows(stopped: &StoppedHost) -> FixtureResult<Vec<DeliveryObservation>> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(stopped.directory().join("interaction.sqlite"))
        .create_if_missing(false)
        .read_only(true)
        .connect()
        .await?;
    let observed: FixtureResult<Vec<DeliveryObservation>> = async {
        let mut transaction = connection.begin().await?;
        let rows = fleet_sync_delivery::test_support::snapshot(&mut TursoScope::new(
            &mut transaction,
            &SqlHost,
        ))
        .await?;
        transaction.commit().await?;
        Ok(rows)
    }
    .await;
    let closed = connection.close().await;
    match (observed, closed) {
        (Ok(rows), Ok(())) => Ok(rows),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error.into()),
        (Err(read), Err(close)) => Err(format!("SDK readback: {read}; close: {close}").into()),
    }
}

pub fn header(
    rows: &[DeliveryObservation],
    category: &str,
    item: &str,
) -> FixtureResult<serde_json::Value> {
    let row = rows
        .iter()
        .find(|row| row.category == category && row.identity == item)
        .ok_or_else(|| format!("{category} row absent for {item}"))?;
    Ok(serde_json::from_str(
        row.content.as_deref().ok_or("custody header absent")?,
    )?)
}
