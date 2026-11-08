//! Own the local Tokio central and synthetic enrollment for D1 Host journeys.
use crate::{
    d1_host_lifecycle::private_directory,
    d1_host_plan::{require_service_directory, require_unaliased_ancestors},
    driver_error::{DriverError, DriverResult},
    driver_records::save_record,
    host_plan::require_deadline,
};
use chrono::{DateTime, FixedOffset};
use collaboration_central_conformance::{
    JwksFixtureServer, access_client_id, fixture_policy, machine_assertion, owner_assertion,
};
use collaboration_central_core::{EnrollMachine, EnrollmentDecision};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use fleet_sync_lease::{LeasePolicy, RegistryConfig};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{io::Write, path::PathBuf, time::Duration};
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct D1CentralPlan {
    pub repository: PathBuf,
    pub root: PathBuf,
    pub deadline: DateTime<FixedOffset>,
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum CentralCommand {
    Enroll {
        service_directory: PathBuf,
        output: PathBuf,
        endpoint: Option<String>,
    },
    Sockets {
        output: PathBuf,
    },
    Stop,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct D1MachineFixture {
    pub allocation: PathBuf,
    pub service_directory: PathBuf,
    pub service_id: String,
    pub peer: String,
    pub receiver: String,
    pub relay: String,
    pub endpoint: String,
    pub assertion: String,
    pub credential: String,
}

fn emit(value: &serde_json::Value) -> DriverResult<()> {
    let mut output = std::io::stdout().lock();
    serde_json::to_writer(&mut output, value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}

fn fixture_error(error: impl std::fmt::Display) -> DriverError {
    DriverError::Io(std::io::Error::other(error.to_string()))
}

impl D1CentralPlan {
    fn validate(&self) -> DriverResult<()> {
        require_deadline(&self.deadline)?;
        let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        if self.repository != repository
            || self.root.parent() != Some(repository.join("tmp/d1-verify/allocation").as_path())
            || self.root.symlink_metadata().is_ok()
        {
            return Err(DriverError::Invalid("fresh D1 central allocation required"));
        }
        require_unaliased_ancestors(&self.root)
    }
    fn output_path(&self, output: &std::path::Path) -> DriverResult<()> {
        if output.parent() != Some(self.root.as_path()) || output.symlink_metadata().is_ok() {
            return Err(DriverError::Invalid(
                "D1 central output must be fresh inside its allocation",
            ));
        }
        Ok(())
    }
}

async fn enroll(
    plan: &D1CentralPlan,
    server: &TokioServer,
    directory: PathBuf,
    output: PathBuf,
    endpoint: Option<String>,
    receiver: fleet_sync_protocol::ReceiverId,
    relay: fleet_sync_protocol::PeerId,
) -> DriverResult<()> {
    require_service_directory(&plan.repository, &directory)?;
    plan.output_path(&output)?;
    if !directory.exists() {
        private_directory(&directory)?;
    }
    let service = collaboration_runtime::load_service_identity(&directory)?;
    let peer = collaboration_runtime::load_installation_peer(&directory)?;
    let credential = uuid::Uuid::now_v7().to_string();
    let expires_at: chrono::DateTime<chrono::Utc> =
        "2033-05-18T03:33:20Z".parse().map_err(fixture_error)?;
    let request = EnrollMachine {
        installation: String::from(service.clone())
            .try_into()
            .map_err(fixture_error)?,
        peer,
        label: format!(
            "D1 {}",
            directory
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("fixture")
        )
        .try_into()
        .map_err(fixture_error)?,
        access_client_id: access_client_id(&credential).map_err(fixture_error)?,
        credential_hash: format!("{:x}", Sha256::digest(credential.as_bytes())),
        expires_at,
    };
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()?
        .post(format!("{}/admin/v1/machines/enroll", server.base_url()))
        .header(
            "Cf-Access-Jwt-Assertion",
            owner_assertion().map_err(fixture_error)?,
        )
        .json(&json!({"kind":"enrollMachine","value":request}))
        .send()
        .await?
        .error_for_status()?;
    let decision: EnrollmentDecision = response.json().await?;
    let EnrollmentDecision::MachineEnrolled {
        peer: admitted,
        authoritative_receiver,
        relay_peer,
        ..
    } = decision
    else {
        return Err(DriverError::Runtime(
            "real central enrollment refused D1 fixture",
        ));
    };
    if admitted != peer || authoritative_receiver != receiver || relay_peer != relay {
        return Err(DriverError::Runtime(
            "D1 authoritative enrollment identity mismatch",
        ));
    }
    let endpoint = endpoint.unwrap_or_else(|| format!("{}/v1/session", server.base_url()));
    let parsed = reqwest::Url::parse(&endpoint).map_err(fixture_error)?;
    if parsed.scheme() != "http"
        || parsed.host_str() != Some("127.0.0.1")
        || parsed.path() != "/v1/session"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(DriverError::Invalid(
            "D1 fixture endpoint must be an owned loopback session edge",
        ));
    }
    save_record(
        &output,
        &D1MachineFixture {
            allocation: plan.root.clone(),
            service_directory: directory.clone(),
            service_id: String::from(service),
            peer: peer.to_string(),
            receiver: receiver.to_string(),
            relay: relay.to_string(),
            endpoint,
            assertion: machine_assertion(&credential).map_err(fixture_error)?,
            credential: format!("Bearer {credential}"),
        },
    )?;
    emit(
        &json!({"kind":"d1Enrolled","serviceDirectory":directory,"fixture":output,"peer":peer.to_string()}),
    )
}

pub async fn run(plan: D1CentralPlan) -> DriverResult<()> {
    plan.validate()?;
    private_directory(&plan.root)?;
    let stores = plan.root.join("stores");
    private_directory(&stores)?;
    let jwks = JwksFixtureServer::start().await.map_err(fixture_error)?;
    let receiver = uuid::Uuid::now_v7()
        .to_string()
        .try_into()
        .map_err(fixture_error)?;
    let relay = uuid::Uuid::now_v7()
        .to_string()
        .try_into()
        .map_err(fixture_error)?;
    let started = TokioServer::start(TokioSettings {
        legacy_organization: collaboration_central_conformance::legacy_organization(),
        root: stores.clone(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: RegistryConfig {
            lease: LeasePolicy {
                lease_duration: Duration::from_secs(14_400),
                renew_interval: Duration::from_secs(900),
            },
            authoritative_receiver: receiver,
            release_unsent_for: Duration::ZERO,
            move_needs_renewal_within: Some(Duration::from_secs(1800)),
        },
        delivery: collaboration_central_tokio::DeliveryConfig { relay_peer: relay },
        holder_answer_timeout: Duration::from_secs(5),
        copy_ack_wait: Duration::from_secs(5),
    })
    .await;
    let server = match started {
        Ok(server) => server,
        Err(error) => {
            jwks.shutdown().await.map_err(fixture_error)?;
            return Err(fixture_error(error));
        }
    };
    let driven: DriverResult<()> = async {
        save_record(&plan.root.join("owner-fixture.json"), &json!({"origin":server.base_url(),"assertion":owner_assertion().map_err(fixture_error)?}))?;
        emit(&json!({"kind":"d1CentralReady","origin":server.base_url(),"stores":stores,"pid":std::process::id()}))?;
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        loop {
            let remaining = (plan.deadline.with_timezone(&chrono::Utc) - chrono::Utc::now()).to_std().map_err(|_| DriverError::TimedOut("D1 central deadline"))?;
            let line = tokio::select! {
                value = tokio::time::timeout(remaining, lines.next_line()) => value.map_err(|_| DriverError::TimedOut("D1 central deadline"))??,
                _ = terminate.recv() => break,
                _ = tokio::signal::ctrl_c() => break,
            };
            let Some(line) = line else { break; };
            match serde_json::from_str::<CentralCommand>(&line)? {
                CentralCommand::Enroll { service_directory, output, endpoint } => enroll(&plan, &server, service_directory, output, endpoint, receiver, relay).await?,
                CentralCommand::Sockets { output } => {
                    plan.output_path(&output)?;
                    let inventory = server.conformance_platform().conformance_socket_inventory().await.map_err(fixture_error)?;
                    let inventory: Vec<_> = inventory.into_iter().map(|owner| json!({"address":owner.address,"admissions":owner.admissions})).collect();
                    save_record(&output, &inventory)?;
                    emit(&json!({"kind":"d1Sockets","output":output}))?;
                }
                CentralCommand::Stop => break,
            }
        }
        Ok(())
    }.await;
    let stopped = server.shutdown().await.map_err(fixture_error);
    let fixture_stopped = jwks.shutdown().await.map_err(fixture_error);
    stopped?;
    fixture_stopped?;
    driven?;
    emit(&json!({"kind":"d1CentralStopped"}))
}
