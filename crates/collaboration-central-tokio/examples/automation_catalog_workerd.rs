//! The same catalog scenario enters through real workerd machine sessions.
#![forbid(unsafe_code)]

use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, run_automation_catalog_conformance,
};
#[path = "../../collaboration-central-conformance/examples/support/workerd_process.rs"]
mod workerd_harness;
use workerd_harness::{WorkerdProcess, WorkerdSettings};

#[tokio::main]
async fn main() -> ConformanceResult<()> {
    let mut arguments = std::env::args().skip(1);
    let node = arguments.next().ok_or("Node executable")?.into();
    let miniflare = arguments
        .next()
        .ok_or("pinned Miniflare entrypoint")?
        .into();
    let persistence = arguments
        .next()
        .ok_or("fresh persistence directory")?
        .into();
    let port = arguments.next().ok_or("loopback port")?.parse::<u16>()?;
    if arguments.next().is_some() {
        return Err("unexpected workerd catalog argument".into());
    }
    drop(std::net::TcpListener::bind(("127.0.0.1", port))?);
    let keys = JwksFixtureServer::start().await?;
    let settings = WorkerdSettings {
        node,
        miniflare,
        persistence,
        port,
        jwks_url: keys.url.clone(),
        relay_peer: "019f0000-0000-7000-8000-000000005240"
            .to_owned()
            .try_into()?,
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    };
    let server = match WorkerdProcess::start(&settings).await {
        Ok(server) => server,
        Err(error) => {
            keys.shutdown().await?;
            return Err(error);
        }
    };
    let base = format!("http://127.0.0.1:{port}");
    let proof = run_automation_catalog_conformance(&ConformanceClient::new(&base)?).await;
    let stopped = server.stop().await;
    let keys = keys.shutdown().await;
    proof?;
    stopped?;
    keys?;
    println!("AUTOMATION_CATALOG_RESULT shared catalog scenario passed; 0 failed; 0 ignored");
    Ok(())
}
