//! Normal owner HTTP publishes a supplied snapshot through the real Fleet and Copy objects.
#![forbid(unsafe_code)]
use collaboration_central_conformance::{
    AdminImportScenario, ConformanceResult, JwksFixtureServer,
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
        return Err("unexpected arguments".into());
    }
    drop(std::net::TcpListener::bind(("127.0.0.1", port))?);
    let jwks = JwksFixtureServer::start().await?;
    let settings = WorkerdSettings {
        fixed_access_time: None,
        legacy_organization: collaboration_central_conformance::legacy_organization(),
        relay_peer: "019f0000-0000-7000-8000-000000005240"
            .to_owned()
            .try_into()?,
        node,
        miniflare,
        persistence,
        port,
        jwks_url: jwks.url.clone(),
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    };
    let base = format!("http://127.0.0.1:{port}");
    let result = async {
        let server = WorkerdProcess::start(&settings).await?;
        let prepared = AdminImportScenario::prepare(&base).await;
        let stopped = server.stop().await;
        stopped?;
        let prepared = prepared?;
        let server = WorkerdProcess::start(&settings).await?;
        let verified = prepared.verify_reopen(&base).await;
        let stopped = server.stop().await;
        stopped?;
        verified
    }
    .await;
    let closed = jwks.shutdown().await;
    closed?;
    result?;
    println!(
        "ADMIN_IMPORT_COLUMNS_RESULT 2 distinct groups passed; 0 failed; 0 ignored (normal owner HTTP, real Fleet continuation and SDK-owned publication)"
    );
    Ok(())
}
