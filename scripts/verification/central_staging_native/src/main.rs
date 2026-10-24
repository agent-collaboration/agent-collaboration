//! Fixture JWKS ownership and the actual Router reqwest upgrade adapter.
//! Secrets enter through named environment values or private fixture files.
use board_fleet_sync::{AccessServiceToken, ReqwestSessionTransport};
use collaboration_central_conformance::{JwksFixtureServer, flip_signature_byte, owner_assertion};
use fleet_sync_protocol::CloseReason;
use fleet_sync_session::{
    ConnectFailure, Endpoint, PeerCredential, SessionTransport, TransportConnection,
};
use serde::Deserialize;
use std::{
    error::Error,
    fs::OpenOptions,
    io::{self, Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    time::Duration,
};
use tokio::io::AsyncReadExt;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpgradeRequest {
    endpoint: String,
    access_id_environment: String,
    access_secret_environment: String,
    credential_environment: String,
}
fn named_value(name: &str) -> Result<String> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err("credential inputs must be environment names".into());
    }
    let value = std::env::var(name).map_err(|_| "credential environment unavailable")?;
    if value.is_empty() || value.contains(['\r', '\n']) {
        return Err("credential environment malformed".into());
    }
    Ok(value)
}
fn private_file(path: &Path, value: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(value.as_bytes())?;
    Ok(())
}
fn emit(value: serde_json::Value) -> Result<()> {
    let mut output = io::stdout().lock();
    serde_json::to_writer(&mut output, &value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}
async fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("fixture") => {
            let root = args.next().ok_or("fixture needs one private directory")?;
            let root = Path::new(&root).canonicalize()?;
            let fixture = JwksFixtureServer::start().await?;
            let owner = owner_assertion()?;
            private_file(&root.join("owner.jwt"), &owner)?;
            private_file(&root.join("forged.jwt"), &flip_signature_byte(&owner)?)?;
            emit(
                serde_json::json!({"kind":"fixtureReady","url":fixture.url,"pid":std::process::id()}),
            )?;
            let mut input = Vec::new();
            tokio::io::stdin().read_to_end(&mut input).await?;
            fixture.shutdown().await?;
            emit(serde_json::json!({"kind":"fixtureStopped"}))
        }
        Some("upgrade") => {
            let mut input = Vec::new();
            io::stdin().read_to_end(&mut input)?;
            let request: UpgradeRequest = serde_json::from_slice(&input)?;
            let transport = ReqwestSessionTransport::new(
                AccessServiceToken::new(
                    &named_value(&request.access_id_environment)?,
                    &named_value(&request.access_secret_environment)?,
                )?,
                Duration::from_secs(15),
                Duration::from_secs(15),
            )?;
            let credential = PeerCredential::new(named_value(&request.credential_environment)?)?;
            let endpoint = Endpoint::new(&request.endpoint)?;
            match transport.connect(&endpoint, &credential).await {
                Ok(connection) => {
                    connection.close(CloseReason::ClientClosed).await;
                    emit(serde_json::json!({"kind":"connected"}))
                }
                Err(ConnectFailure::CredentialRejected(reason)) => {
                    emit(serde_json::json!({"kind":"credentialRejected","reason":reason}))
                }
                Err(ConnectFailure::InvalidEndpoint) => {
                    emit(serde_json::json!({"kind":"invalidEndpoint"}))
                }
                Err(ConnectFailure::InvalidCredential) => {
                    emit(serde_json::json!({"kind":"invalidCredential"}))
                }
                Err(ConnectFailure::Unavailable) => emit(serde_json::json!({"kind":"unavailable"})),
            }
        }
        _ => Err("usage: central-staging-probe fixture PRIVATE_ROOT | upgrade (JSON stdin)".into()),
    }
}
#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("staging probe failed; no credential or endpoint detail emitted");
        std::process::exit(1);
    }
}
