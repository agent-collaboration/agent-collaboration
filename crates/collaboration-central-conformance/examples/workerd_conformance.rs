#![forbid(unsafe_code)]
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, run_copy_conformance, run_numeric_conformance,
    run_session_conformance,
};
#[tokio::main]
async fn main() -> ConformanceResult<()> {
    let mut args = std::env::args().skip(1);
    let url = args.next().ok_or("base URL argument")?;
    let client = ConformanceClient::new(&url)?;
    run_copy_conformance(&client).await?;
    run_numeric_conformance(&client).await?;
    run_session_conformance(&client).await?;
    println!(
        "workerd shared conformance: copy, exact numeric transport and session/transfer groups passed"
    );
    Ok(())
}
