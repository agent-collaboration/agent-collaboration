#[allow(
    dead_code,
    unused_imports,
    reason = "each fixture binary uses a different subset of the shared consumer harness"
)]
#[path = "lib.rs"]
mod consumer_fixture;
#[tokio::main]
async fn main() -> crate::consumer_fixture::TestResult {
    let directory = std::env::args_os()
        .nth(1)
        .ok_or("usage: fleet-sync-test-consumer <state-directory> [port]")?;
    if std::env::args().nth(2).as_deref() == Some("link-probe") {
        let directory = std::path::Path::new(&directory);
        std::fs::hard_link(directory.join("source"), directory.join("destination"))?;
        std::fs::File::open(directory)?.sync_all()?;
        println!("LINK_READY");
        std::future::pending::<()>().await;
        return Ok(());
    }
    if let Some(mode) = std::env::args().nth(2)
        && matches!(mode.as_str(), "move-aside" | "retry-move-aside")
    {
        let destination = std::env::args_os().nth(3).ok_or("move needs destination")?;
        let database = fleet_protocol::DatabaseId::new(
            std::env::args()
                .nth(4)
                .ok_or("move needs database")?
                .parse()?,
        )?;
        let phase = std::env::args()
            .nth(5)
            .map(|value| serde_json::from_str(&value))
            .transpose()?;
        return crate::consumer_fixture::run_move(
            std::path::Path::new(&directory),
            std::path::Path::new(&destination),
            database,
            phase,
            mode == "retry-move-aside",
        )
        .await;
    }
    if std::env::args().nth(2).as_deref() == Some("writer") {
        let endpoint = std::env::args().nth(3).ok_or("writer needs endpoint")?;
        let mode = std::env::args().nth(4).ok_or("writer needs mode")?;
        let database = std::env::args()
            .nth(5)
            .map(|value| value.parse::<durable_steps::Uuid>())
            .transpose()?
            .map(fleet_protocol::DatabaseId::new)
            .transpose()?;
        return crate::consumer_fixture::run_writer(
            std::path::Path::new(&directory),
            &endpoint,
            &mode,
            database,
        )
        .await;
    }
    let port = std::env::args()
        .nth(2)
        .map(|value| value.parse())
        .transpose()?
        .unwrap_or(0);
    let receiver = std::env::args()
        .nth(3)
        .map(|value| value.parse())
        .transpose()?
        .unwrap_or(3);
    let lease_ms = std::env::args()
        .nth(4)
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(600_000);
    let move_renewal_ms = std::env::args()
        .nth(5)
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(60_000);
    crate::consumer_fixture::run_server_with_policy(
        std::path::Path::new(&directory),
        port,
        receiver,
        std::time::Duration::from_millis(lease_ms),
        std::time::Duration::from_millis(move_renewal_ms),
    )
    .await
}
