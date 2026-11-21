struct Receipt {
    caller: i64,
    answer: String,
}

fn main() {
    let _query = sqlx::query_as!(Receipt, "SELECT caller, answer FROM workspace_request_log");
}
