struct Receipt {
    caller: String,
    answer: String,
}

fn main() {
    let _query = sqlx::query_as!(Receipt, "SELECT caller FROM workspace_request_log");
}
