//! Corrupt only a stopped fixture's local evidence, keeping its genuine settled project unchanged.
use super::{
    cutover_host_fixture::CutoverHostFixture, offline_cli_fixture::TestResult,
    recovery_rows::require,
};
use board_staging_cutover::{NativeProjectSnapshot, SelectedWriterFile};
use fleet_sync_protocol::SqlValue;
use message_board::ProjectId;
use sqlx::Connection;
use sqlx_turso::{TursoConnectOptions, TursoConnection};

pub(super) async fn corrupt(
    fixture: &CutoverHostFixture,
    project: &ProjectId,
    wrong_target: bool,
) -> TestResult {
    let mut host = fixture
        .acquire()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let mut installed = NativeProjectSnapshot::open(
        &mut host,
        SelectedWriterFile {
            project: project.clone(),
            peer: fixture.peer,
            file_epoch: fleet_sync_protocol::Epoch::new(2)?,
        },
    )
    .await?;
    let read = async {
        let mut frontier = Vec::new();
        while let Some(page) = installed.next_page().await? {
            if page.contents.table == "handoff_frontier" {
                frontier.extend(page.contents.rows);
            }
        }
        require(
            frontier.len() == 1
                && frontier.first().and_then(|row| row.get(1)) == Some(&SqlValue::Integer(1)),
            "fixture must expose an actual terminal settled frontier before corrupting custody",
        )
    }
    .await;
    let closed = installed.close().await;
    read?;
    closed?;
    let path = host.collaboration_directory().join("interaction.sqlite");
    let mut connection = TursoConnection::connect_with(
        &TursoConnectOptions::new()
            .filename(path)
            .create_if_missing(false),
    )
    .await?;
    let changed = async {
        let items: Vec<String> = sqlx::query_scalar(
            "SELECT push_id FROM router_pushes WHERE retention='mailTerminalAge'",
        )
        .fetch_all(&mut connection)
        .await?;
        let [item] = items.as_slice() else {
            return Err("expected exactly one real recovered push".into());
        };
        let changed = if wrong_target {
            sqlx::query("UPDATE router_pushes SET target_session_id=? WHERE push_id=?")
                .bind(uuid::Uuid::now_v7().to_string())
                .bind(item)
                .execute(&mut connection)
                .await?
                .rows_affected()
        } else {
            sqlx::query("DELETE FROM _fleet_sync_inbox_items WHERE item_id=?")
                .bind(item)
                .execute(&mut connection)
                .await?
                .rows_affected()
        };
        require(
            changed == 1,
            "fault must change exactly one actual custody row",
        )
    }
    .await;
    let closed = connection.close().await;
    changed?;
    closed?;
    Ok(())
}
