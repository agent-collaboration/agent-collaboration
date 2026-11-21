fn main() {
    let _query = sqlx::query!("SELECT caller FROM workspace_request_log WHERE request_id = ?", "id", "extra");
}
