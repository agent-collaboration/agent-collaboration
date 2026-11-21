#![expect(
    clippy::unwrap_used,
    reason = "fixture inputs and setup failures must fail the calling test"
)]
use sqlx::{Connection, SqliteConnection, migrate::Migrator};

pub fn key(number: u32) -> String {
    format!("01890f2e-7b4c-7cc0-98c4-{number:012}")
}
pub async fn database(migrator: &Migrator) -> SqliteConnection {
    let mut connection = SqliteConnection::connect("sqlite::memory:").await.unwrap();
    migrator.run(&mut connection).await.unwrap();
    sqlx::query("INSERT INTO signin_organization_identity VALUES (?,'statement',0,0)")
        .bind(key(1))
        .execute(&mut connection)
        .await
        .unwrap();
    connection
}
pub async fn wake(connection: &mut SqliteConnection, number: u32, state: &str, due: &str) {
    sqlx::query("INSERT INTO automation_wakes VALUES (?,'target',?,'wake',?,'1970-01-01T00:00:00.000Z','1970-01-01T00:00:00.000Z')")
        .bind(key(number)).bind(due).bind(state).execute(connection).await.unwrap();
}
pub async fn attempt(
    connection: &mut SqliteConnection,
    number: u32,
    kind: &str,
    subject: &str,
    state: &str,
    class: &str,
) {
    sqlx::query("INSERT INTO engine_work_attempts VALUES (?,?,?,?,1,'1970-01-01T00:10:00.000Z',?,NULL,'1970-01-01T00:00:00.000Z','1970-01-01T00:00:00.000Z',NULL,'1970-01-01T00:00:00.000Z','1970-01-01T00:00:00.000Z')")
        .bind(key(number)).bind(kind).bind(subject).bind(state).bind(class).execute(connection).await.unwrap();
}
