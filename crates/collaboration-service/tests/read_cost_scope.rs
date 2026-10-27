#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! A measured future excludes concurrent native SQL outside its subscriber.
#![allow(clippy::panic_in_result_fn)]
#![deny(clippy::too_many_lines)]
#[allow(dead_code)] // Other cost-proof APIs share this source, but are not exercised here.
#[path = "support/read_cost_counter.rs"]
mod read_cost_counter;

use read_cost_counter::ReadCostObservation;
use sqlx::{ConnectOptions, Connection, Executor};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
async fn data_budget_ignores_real_pragma_but_detects_an_extra_select() -> TestResult {
    let mut connection = sqlx_turso::TursoConnectOptions::new().connect().await?;
    let first = ReadCostObservation::default();
    first.observe(connection.execute("SELECT 1")).await?;
    let second = ReadCostObservation::default();
    second
        .observe(async {
            connection.execute("PRAGMA user_version").await?;
            connection.execute("SELECT 1").await?;
            Ok::<_, sqlx::Error>(())
        })
        .await?;
    assert_eq!(
        first.data_statement_counts(),
        second.data_statement_counts()
    );
    assert_eq!(first.cost().statements, 1);
    assert_eq!(
        second.cost().statements,
        2,
        "the legacy counter must retain non-data work"
    );
    second.observe(connection.execute("SELECT 2")).await?;
    assert_ne!(
        first.data_statement_counts(),
        second.data_statement_counts(),
        "an extra data query must change the exact budget"
    );
    connection.close().await?;
    Ok(())
}

#[tokio::test]
async fn scoped_read_cost_excludes_real_concurrent_sql_and_separates_observations() -> TestResult {
    read_cost_counter::install();
    let mut measured = sqlx_turso::TursoConnectOptions::new().connect().await?;
    let mut unrelated = sqlx_turso::TursoConnectOptions::new().connect().await?;
    let (release, released) = tokio::sync::oneshot::channel();
    let (completed, completion) = tokio::sync::oneshot::channel();
    let background = tokio::spawn(async move {
        released.await?;
        unrelated.execute("SELECT 991").await?;
        completed.send(()).map_err(|()| "measurement ended early")?;
        unrelated.close().await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    });
    let first = ReadCostObservation::default();
    let before_background = read_cost_counter::current_cost();
    first
        .observe(async {
            measured.execute("SELECT 1").await?;
            release.send(()).map_err(|()| "background ended early")?;
            completion.await?;
            measured.execute("SELECT 2").await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        })
        .await?;
    background.await??;
    let unrelated_cost = read_cost_counter::current_cost().since(before_background);
    assert_eq!(
        unrelated_cost.statements, 1,
        "negative control did not execute real unrelated SQL"
    );
    assert_eq!(first.cost().statements, 2);
    assert_eq!(first.statement_kinds().len(), 2);
    assert!(first.statements().iter().all(|sql| !sql.contains("991")));
    let second = ReadCostObservation::default();
    second.observe(measured.execute("SELECT 3")).await?;
    assert_eq!(second.cost().statements, 1);
    assert_eq!(
        first.cost().statements,
        2,
        "later work changed an earlier observation"
    );
    measured.close().await?;
    Ok(())
}
