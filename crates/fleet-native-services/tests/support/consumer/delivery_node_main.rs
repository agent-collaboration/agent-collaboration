#[allow(
    dead_code,
    unused_imports,
    reason = "each fixture binary uses a different subset of the shared consumer harness"
)]
#[path = "lib.rs"]
mod consumer_fixture;
#[tokio::main]
async fn main() -> crate::consumer_fixture::TestResult {
    let mut arguments = std::env::args().skip(1);
    let directory = arguments.next().ok_or("missing node directory")?;
    let peer = arguments.next().ok_or("missing node peer")?.parse()?;
    crate::consumer_fixture::delivery_node::run(std::path::Path::new(&directory), peer).await
}
