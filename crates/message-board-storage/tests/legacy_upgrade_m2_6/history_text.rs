#![allow(clippy::unwrap_used)]
use super::board_behavior_support::*;
use message_board::*;
use message_board_storage::BoardStore;
use sqlx::{Connection, SqliteConnection};

#[tokio::test]
async fn a_stored_control_character_reads_back_escaped_instead_of_raw() {
    // Arrange: a row that predates the write-boundary rejection.
    let path = database_path("stored-control-escape");
    let mut store = BoardStore::open(&path).await.unwrap();
    let fixture = create_fixture(&mut store).await;
    let posted = post(
        &mut store,
        Placement::Topic {
            topic_id: fixture.topic_id.clone(),
        },
        actor("owner"),
        "before after",
        Vec::new(),
    )
    .await
    .message;
    store.close().await.unwrap();
    let mut connection = SqliteConnection::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    sqlx::query("UPDATE board_messages SET text=? WHERE message_id=?")
        .bind("before\u{0007}after")
        .bind(posted.message_id.as_str())
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();

    // Act.
    let mut store = BoardStore::open(&path).await.unwrap();
    let read = store
        .show_message(MessageShowRequest {
            message_id: posted.message_id.clone(),
        })
        .await
        .unwrap();

    // Assert: the reader sees an escape, and the row still parses as JSON text.
    assert_eq!(read.0.text.as_str(), "before\\u0007after");
    assert!(serde_json::to_string(&read.0).unwrap().contains("before"));
    store.close().await.unwrap();
    std::fs::remove_file(path).unwrap();
}
