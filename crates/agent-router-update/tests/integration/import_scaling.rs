//! Retained, synthetic-only measurements for the offline import's actual CLI path.
use crate::support;
use sqlx::Connection as _;

use message_board::PostId;
use std::{
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};
use support::*;

async fn grow_fixture(source: &Path, posts: usize) -> TestResult {
    let mut connection = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(source)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await?;
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&mut connection)
        .await?;
    let original = usize::try_from(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM board_messages")
            .fetch_one(&mut connection)
            .await?,
    )?;
    let mut sequence: i64 = sqlx::query_scalar("SELECT MAX(activity_sequence) FROM board_activity")
        .fetch_one(&mut connection)
        .await?;
    let mut transaction = connection.begin().await?;
    for _ in original..posts {
        let id = PostId::generate();
        sequence += 1;
        sqlx::query("INSERT INTO board_messages(message_id,topic_id,board_id,root_id,actor_key,acting_for_key,text,posted_from_activity) SELECT ?,topic_id,board_id,NULL,actor_key,NULL,'Synthetic scaling post.',NULL FROM board_messages WHERE root_id IS NULL ORDER BY message_id LIMIT 1").bind(id.as_str()).execute(&mut *transaction).await.map(|result|result.rows_affected())?;
        sqlx::query("INSERT INTO board_threads(root_id,state) VALUES (?,'unresolved')")
            .bind(id.as_str())
            .execute(&mut *transaction)
            .await
            .map(|result| result.rows_affected())?;
        sqlx::query("INSERT INTO board_activity(activity_sequence,project_id,board_id,topic_id,root_id,kind,actor_key,message_id,participant_key,participant_role,replaced_participant_key) SELECT ?,b.project_id,m.board_id,m.topic_id,NULL,'mainMessageCreated',m.actor_key,m.message_id,NULL,NULL,NULL FROM board_messages m JOIN project_boards b ON b.board_id=m.board_id WHERE m.message_id=?").bind(sequence).bind(id.as_str()).execute(&mut *transaction).await.map(|result|result.rows_affected())?;
    }
    sqlx::query("UPDATE activity_checkpoint SET last_sequence=? WHERE singleton=1")
        .bind(sequence)
        .execute(&mut *transaction)
        .await
        .map(|result| result.rows_affected())?;
    transaction.commit().await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM board_messages")
            .fetch_one(&mut connection)
            .await?,
        i64::try_from(posts)?
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_optional(&mut connection)
            .await?
            .is_none()
    );
    connection.close().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "manual 893 scaling measurement; runs the real native import at 1k/5k/10k posts"]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn offline_import_phase_growth_at_three_synthetic_sizes() -> TestResult {
    for posts in [1000, 5000, 10000] {
        // Arrange: only retained synthetic data is enlarged, in an isolated copy.
        let (directory, source) = fixture_copy("legacy-complete.sqlite")?;
        grow_fixture(&source, posts).await?;
        let before = std::fs::read(&source)?;
        create_host_identity(directory.path())?;
        let report = directory.path().join("report.json");

        // Act: identical offline command and native path to the private run.
        eprintln!("scaling begin posts={posts}");
        let started = Instant::now();
        let status = tokio::time::timeout(
            Duration::from_secs(1800),
            tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-router-update"))
                .arg("board-import")
                .arg("--source")
                .arg(&source)
                .arg("--router-directory")
                .arg(directory.path())
                .arg("--report")
                .arg(&report)
                .args([
                    "--owner-human-id",
                    "fixture-owner",
                    "--machine-label",
                    "scaling fixture",
                    "--dry-run",
                ])
                .kill_on_drop(true)
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .status(),
        )
        .await??;
        eprintln!(
            "scaling end posts={posts} elapsed_ms={}",
            started.elapsed().as_millis()
        );

        // Assert: timing does not weaken transformation or source preservation.
        assert!(status.success());
        let result: serde_json::Value = serde_json::from_slice(&std::fs::read(report)?)?;
        assert_eq!(result["kind"], "dryRun");
        let actual = result["report"]["projects"]
            .as_array()
            .ok_or("projects")?
            .iter()
            .map(|project| project["posts"].as_u64().ok_or("posts"))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .sum::<u64>();
        assert_eq!(actual, u64::try_from(posts)?);
        assert_eq!(std::fs::read(&source)?, before);
    }
    Ok(())
}
