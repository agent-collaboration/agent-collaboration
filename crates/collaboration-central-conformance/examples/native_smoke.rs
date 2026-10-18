#![forbid(unsafe_code)]
use collaboration_central_conformance::{ConformanceClient, ConformanceResult, run_default_smoke};
#[tokio::main]
async fn main() -> ConformanceResult<()> {
    let mut args = std::env::args().skip(1);
    let url = args.next().ok_or("base URL argument")?;
    run_default_smoke(&ConformanceClient::new(&url)?).await
}
