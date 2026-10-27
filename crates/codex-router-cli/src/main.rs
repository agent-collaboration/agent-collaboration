#![expect(clippy::large_futures, reason = "lint pass follows the crate split")]
#[tokio::main]
async fn main() {
    std::process::exit(codex_router_cli::run_async().await);
}
