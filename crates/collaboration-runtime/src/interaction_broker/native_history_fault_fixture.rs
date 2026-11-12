//! Fault SQL runs on a second connection to the broker's native file.
type HistoryDatabase = sqlx_turso::Turso;
type HistoryConnection = sqlx_turso::TursoConnection;
type HistoryConnectOptions = sqlx_turso::TursoConnectOptions;

pub(super) struct IndependentNativeHistoryRead {
    connection: HistoryConnection,
}
impl IndependentNativeHistoryRead {
    pub(super) async fn open(path: &std::path::Path) -> Result<Self, sqlx::Error> {
        // A second connection uses the existing native engine, not a second
        // store owner. Opposed read-only engine admission is invalid beside
        // its live writer; this private observer exposes only SELECT.
        let options = HistoryConnectOptions::new()
            .filename(path)
            .create_if_missing(false);
        let connection = <HistoryConnection as sqlx::Connection>::connect_with(&options).await?;
        Ok(Self { connection })
    }

    pub(super) async fn record_json(&mut self, request: &str) -> Result<String, sqlx::Error> {
        sqlx::query_scalar::<HistoryDatabase, String>(
            "SELECT record_json FROM typed_interaction_history WHERE request_id=?",
        )
        .bind(request)
        .fetch_one(&mut self.connection)
        .await
    }

    pub(super) async fn close(self) -> Result<(), sqlx::Error> {
        <HistoryConnection as sqlx::Connection>::close(self.connection).await
    }
}

/// Fault SQL on a second native connection, each call one committed transaction.
pub(super) struct NativeHistoryFaults {
    connection: tokio::sync::Mutex<HistoryConnection>,
}
impl NativeHistoryFaults {
    pub(super) async fn open(path: &std::path::Path) -> Result<Self, sqlx::Error> {
        let options = HistoryConnectOptions::new()
            .filename(path)
            .create_if_missing(false);
        let connection = <HistoryConnection as sqlx::Connection>::connect_with(&options).await?;
        Ok(Self {
            connection: tokio::sync::Mutex::new(connection),
        })
    }

    pub(super) async fn execute_all(
        &self,
        statements: &'static [&'static str],
    ) -> Result<(), sqlx::Error> {
        let mut connection = self.connection.lock().await;
        let mut transaction = sqlx::Connection::begin(&mut *connection).await?;
        for &statement in statements {
            sqlx::raw_sql(statement).execute(&mut *transaction).await?;
        }
        transaction.commit().await
    }

    pub(super) async fn text(&self, statement: &'static str) -> Result<String, sqlx::Error> {
        sqlx::query_scalar::<HistoryDatabase, String>(statement)
            .fetch_one(&mut *self.connection.lock().await)
            .await
    }

    pub(super) async fn integer(&self, statement: &'static str) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar::<HistoryDatabase, i64>(statement)
            .fetch_one(&mut *self.connection.lock().await)
            .await
    }
}
