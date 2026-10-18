#![forbid(unsafe_code)]
use clap::Parser;
use collaboration_central_local::{LocalResult, LocalServer, LocalSettings};
#[derive(Parser)]
#[command(about = "Local central: isolated Turso objects and machine WebSockets")]
struct Arguments {
    #[arg(long)]
    root: std::path::PathBuf,
    #[arg(long, default_value = "127.0.0.1:8081")]
    listen: std::net::SocketAddr,
    #[arg(long)]
    access_team_domain: String,
    #[arg(long)]
    access_session_audience: String,
    #[arg(long)]
    access_admin_audience: String,
    #[arg(long)]
    access_owner_identity: String,
    #[arg(long)]
    access_jwks_url: String,
}
#[tokio::main]
async fn main() -> LocalResult<()> {
    let args = Arguments::parse();
    let server = LocalServer::start_on(
        LocalSettings {
            root: args.root,
            access_policy: collaboration_central_core::AccessPolicy {
                team_domain: args.access_team_domain,
                session_audience: args.access_session_audience,
                admin_audience: args.access_admin_audience,
                owner_identity: args.access_owner_identity,
            },
            jwks_url: args.access_jwks_url,
        },
        args.listen,
    )
    .await?;
    println!("Local central listening at {}", server.base_url());
    tokio::signal::ctrl_c().await?;
    server.shutdown().await
}
