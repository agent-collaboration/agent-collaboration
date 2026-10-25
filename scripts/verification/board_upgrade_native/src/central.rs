//! Own an isolated real Tokio central; export readback through its owning process.
//!
//! Usage: board-upgrade-central REQUEST.json. The private request names a system-temp
//! root, a fresh fixture-assertion file, and project UUIDs. JSON lines on stdin are
//! {"kind":"inspect","output":"..."} or {"kind":"stop","output":"..."}.
//! Ready/inspection/stopped receipts go to stdout; no assertion value is printed.

use collaboration_central_conformance::{JwksFixtureServer, fixture_policy, owner_assertion};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use fleet_sync_lease::{LeasePolicy, RegistryConfig};
use fleet_sync_protocol::{DatabaseId, ReceiverId};
use serde::{Deserialize, Serialize};
use sqlx_turso::sqlx::{ConnectOptions, Connection, Row};
use std::{
    error::Error,
    fs::{OpenOptions, Permissions},
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, BufReader};

type DriverResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CentralRequest {
    root: PathBuf,
    assertion_file: PathBuf,
    projects: Vec<DatabaseId>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Command {
    Inspect { output: PathBuf },
    Stop { output: PathBuf },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CopyReadback {
    database: DatabaseId,
    record: Vec<String>,
    seed: Vec<String>,
    summary: Vec<String>,
    posts: Vec<String>,
    staged: Vec<String>,
}

#[derive(Serialize)]
struct CentralReadback {
    copies: Vec<CopyReadback>,
    workflows: Vec<String>,
    directory: Vec<String>,
}

fn write_new(path: &Path, bytes: &[u8]) -> DriverResult<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    Ok(())
}

fn emit(value: serde_json::Value) -> DriverResult<()> {
    let mut output = io::stdout().lock();
    serde_json::to_writer(&mut output, &value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}

fn validate_output(root: &Path, output: &Path) -> DriverResult<()> {
    let parent = output
        .parent()
        .ok_or_else(|| io::Error::other("output parent missing"))?
        .canonicalize()?;
    let evidence = root
        .parent()
        .ok_or_else(|| io::Error::other("root parent missing"))?;
    if !output.is_absolute() || !parent.starts_with(evidence) || output.exists() {
        return Err(io::Error::other("output must be fresh inside private run evidence").into());
    }
    Ok(())
}

async fn values(root: &Path, object: &str, sql: &'static str) -> DriverResult<Vec<String>> {
    let path = root.join(format!("{object}.db"));
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false)
        .connect()
        .await?;
    let result = sqlx_turso::sqlx::query::<sqlx_turso::Turso>(sql)
        .fetch_all(&mut connection)
        .await;
    connection.close().await?;
    Ok(result?
        .iter()
        .map(|row| row.try_get("value"))
        .collect::<Result<Vec<String>, _>>()?)
}

async fn inspect(request: &CentralRequest, output: &Path) -> DriverResult<()> {
    validate_output(&request.root, output)?;
    let mut copies = Vec::new();
    for database in &request.projects {
        let object = format!("copy-{database}");
        let tables = values(
            &request.root,
            &object,
            "SELECT name AS value FROM sqlite_schema WHERE type='table' AND name='posts'",
        )
        .await?;
        copies.push(CopyReadback {
            database: *database,
            record: values(
                &request.root,
                &object,
                "SELECT record AS value FROM _fleet_sync_copy",
            )
            .await?,
            seed: values(
                &request.root,
                &object,
                "SELECT record AS value FROM _fleet_sync_seed",
            )
            .await?,
            summary: values(
                &request.root,
                &object,
                "SELECT record AS value FROM _copy_summary",
            )
            .await?,
            posts: if tables.is_empty() {
                Vec::new()
            } else {
                values(
                    &request.root,
                    &object,
                    "SELECT CAST(count(*) AS TEXT) AS value FROM posts",
                )
                .await?
            },
            staged: values(
                &request.root,
                &object,
                "SELECT CAST(count(*) AS TEXT) AS value FROM _fleet_sync_seed_pages",
            )
            .await?,
        });
    }
    let snapshot = CentralReadback {
        copies,
        workflows: values(&request.root, "fleet", "SELECT json_object('importId',import_id,'project',project_id,'digest',digest,'header',header,'phase',phase) AS value FROM admin_imports ORDER BY project_id").await?,
        directory: values(&request.root, "fleet", "SELECT copy_summary AS value FROM project_directory WHERE copy_summary IS NOT NULL ORDER BY project_id").await?,
    };
    write_new(output, &serde_json::to_vec_pretty(&snapshot)?)
}

async fn commands(request: &CentralRequest) -> DriverResult<Option<PathBuf>> {
    let mut input = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = input.next_line().await? {
        match serde_json::from_str(&line)? {
            Command::Inspect { output } => {
                inspect(request, &output).await?;
                emit(serde_json::json!({"kind":"inspected","output":output}))?;
            }
            Command::Stop { output } => {
                validate_output(&request.root, &output)?;
                return Ok(Some(output));
            }
        }
    }
    Ok(None)
}

#[tokio::main]
async fn main() -> DriverResult<()> {
    let path = std::env::args_os().nth(1).ok_or_else(|| {
        io::Error::other("usage: board-upgrade-central REQUEST.json (private system-temp state)")
    })?;
    if path == "--help" {
        println!(
            "board-upgrade-central REQUEST.json\nOwn a real isolated Tokio central with fixture Access. The private request has root, assertionFile and nonempty projects. Stdin JSON lines: {{\"kind\":\"inspect\",\"output\":\"...\"}} or {{\"kind\":\"stop\",\"output\":\"...\"}}. Ready/inspected/stopped receipts go to stdout. Stop drains the owner before final readback; EOF also drains. State and outputs must remain in the supplied private system-temp task."
        );
        return Ok(());
    }
    let request: CentralRequest = serde_json::from_slice(&std::fs::read(path)?)?;
    let root = request.root.canonicalize()?;
    let temp = std::env::temp_dir().canonicalize()?;
    if request.root != root
        || (!root.starts_with(&temp) && !root.starts_with("/private/tmp"))
        || root == temp
        || root == Path::new("/private/tmp")
        || std::fs::metadata(&root)?.permissions().mode() & 0o077 != 0
        || request.projects.is_empty()
    {
        return Err(io::Error::other("root must be a private, canonical system-temp task directory; projects cannot be empty").into());
    }
    validate_output(&root, &request.assertion_file)?;
    write_new(&request.assertion_file, owner_assertion()?.as_bytes())?;
    std::fs::set_permissions(&request.assertion_file, Permissions::from_mode(0o600))?;
    let jwks = JwksFixtureServer::start().await?;
    let server = match TokioServer::start(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: request.root.clone(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: RegistryConfig {
            lease: LeasePolicy {
                lease_duration: Duration::from_secs(14_400),
                renew_interval: Duration::from_secs(900),
            },
            authoritative_receiver: ReceiverId::try_from(
                "019f0000-0000-7000-8000-000000000010".to_owned(),
            )?,
            release_unsent_for: Duration::ZERO,
            move_needs_renewal_within: Some(Duration::from_secs(1800)),
        },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await
    {
        Ok(server) => server,
        Err(error) => {
            jwks.shutdown().await?;
            return Err(error.into());
        }
    };
    let driven = async {
        emit(serde_json::json!({"kind":"ready","url":server.base_url(),"pid":std::process::id()}))?;
        commands(&request).await
    }
    .await;
    let stopped = server.shutdown().await;
    let fixture_stopped = jwks.shutdown().await;
    stopped?;
    fixture_stopped?;
    if let Some(output) = driven? {
        inspect(&request, &output).await?;
        emit(serde_json::json!({"kind":"stopped","output":output}))?;
    }
    Ok(())
}
