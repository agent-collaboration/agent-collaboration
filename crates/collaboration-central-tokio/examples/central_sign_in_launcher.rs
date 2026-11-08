//! Conformance only: launch Tokio central with its private sign-in link for a
//! joint sign-in + central harness. `central serve` is untouched.
//!
//! Settings come from flags, or from `CENTRAL_LAUNCHER_<FLAG>` environment
//! variables (upper case, `-` as `_`): `--root`, `--socket`, `--issuer`,
//! `--audience`, `--admin-audience`, `--public-origin`, `--jwks-url`,
//! `--service-id`, `--legacy-organization`, `--fixed-time-ms`, and
//! `--organization` (repeatable; the legacy organization is always pinned).
//!
//! stdout carries exactly one line, at readiness:
//! `{"baseUrl":…,"socket":…}`. Everything else goes to stderr.
//!
//! stdin takes one JSON command per line; each is answered by one JSON line on
//! stderr carrying `"answer"`:
//! - `{"setTimeMs":N,"organizations":["<org>",…]}` moves each organization's
//!   Fleet clock through central's own conformance runtime route;
//! - `{"observe":"<org>"}` reads the owner-only, read-only organization
//!   observer.
//!
//! Access is the conformance fixture, started in this process, so the caller
//! never mints an assertion. SIGTERM or SIGINT shuts down and removes the
//! socket.
use collaboration_central_conformance::{
    ConformanceResult, JwksFixtureServer, RuntimeControl, RuntimeControlRequest, fixture_policy,
    owner_assertion,
};
use collaboration_central_core::{ORGANIZATION_RECORDS_PATH, ObjectAddress, RegistryTime};
use collaboration_central_tokio::{DeliveryConfig, SignInLinkSettings, TokioServer, TokioSettings};
use fleet_identity_protocol::OrganizationId;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

const SINGLE: [&str; 10] = [
    "root",
    "socket",
    "issuer",
    "audience",
    "admin-audience",
    "public-origin",
    "jwks-url",
    "service-id",
    "legacy-organization",
    "fixed-time-ms",
];

struct LauncherArguments {
    single: BTreeMap<&'static str, String>,
    organizations: Vec<String>,
}

impl LauncherArguments {
    fn parse() -> ConformanceResult<Self> {
        let mut single = BTreeMap::new();
        let mut organizations = Vec::new();
        let mut arguments = std::env::args().skip(1);
        while let Some(flag) = arguments.next() {
            let name = flag
                .strip_prefix("--")
                .ok_or_else(|| format!("unexpected argument {flag}"))?;
            let value = arguments
                .next()
                .ok_or_else(|| format!("--{name} needs a value"))?;
            if name == "organization" {
                organizations.push(value);
            } else {
                let known = SINGLE
                    .iter()
                    .find(|known| **known == name)
                    .ok_or_else(|| format!("unknown flag --{name}"))?;
                if single.insert(*known, value).is_some() {
                    return Err(format!("--{name} given twice").into());
                }
            }
        }
        for name in SINGLE {
            if !single.contains_key(name) {
                let variable =
                    format!("CENTRAL_LAUNCHER_{}", name.to_uppercase().replace('-', "_"));
                if let Ok(value) = std::env::var(&variable) {
                    single.insert(name, value);
                }
            }
        }
        if organizations.is_empty()
            && let Ok(value) = std::env::var("CENTRAL_LAUNCHER_ORGANIZATIONS")
        {
            organizations = value.split(',').map(str::to_owned).collect();
        }
        Ok(Self {
            single,
            organizations,
        })
    }

    fn get(&self, name: &str) -> ConformanceResult<String> {
        self.single
            .get(name)
            .cloned()
            .ok_or_else(|| format!("--{name} is required").into())
    }
}

fn set_time(organization: OrganizationId, millis: i64) -> ConformanceResult<RuntimeControlRequest> {
    Ok(RuntimeControlRequest {
        address: ObjectAddress::Fleet { organization },
        command: RuntimeControl::SetTime {
            now: RegistryTime(
                chrono::DateTime::from_timestamp_millis(millis).ok_or("time out of range")?,
            ),
        },
    })
}

#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum LauncherCommand {
    #[serde(rename_all = "camelCase")]
    SetTime {
        set_time_ms: i64,
        organizations: Vec<OrganizationId>,
    },
    Observe {
        observe: OrganizationId,
    },
}

/// Answer one stdin command through central's own owner-only routes.
async fn answer(client: &reqwest::Client, base_url: &str, line: &str) -> Value {
    let command: LauncherCommand = match serde_json::from_str(line) {
        Ok(command) => command,
        Err(error) => return json!({"answer":"refused","error":error.to_string()}),
    };
    let result: ConformanceResult<Value> = async {
        match command {
            LauncherCommand::SetTime {
                set_time_ms,
                organizations,
            } => {
                for organization in &organizations {
                    client
                        .post(format!("{base_url}/admin/v1/conformance/runtime"))
                        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
                        .json(&set_time(organization.clone(), set_time_ms)?)
                        .send()
                        .await?
                        .error_for_status()?;
                }
                Ok(json!({"answer":"timeSet","setTimeMs":set_time_ms,"organizations":organizations}))
            }
            LauncherCommand::Observe { observe } => {
                let response = client
                    .post(format!("{base_url}{ORGANIZATION_RECORDS_PATH}"))
                    .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
                    .json(&json!({"organization":observe}))
                    .send()
                    .await?
                    .error_for_status()?;
                let observation: Value = response.json().await?;
                Ok(json!({"answer":"observed","organization":observe,"observation":observation}))
            }
        }
    }
    .await;
    result.unwrap_or_else(|error| json!({"answer":"failed","error":error.to_string()}))
}

#[tokio::main]
async fn main() -> ConformanceResult<()> {
    let arguments = LauncherArguments::parse()?;
    let legacy: OrganizationId = arguments.get("legacy-organization")?.try_into()?;
    let fixed_time: i64 = arguments.get("fixed-time-ms")?.parse()?;
    let mut pinned: Vec<OrganizationId> = vec![legacy.clone()];
    for organization in &arguments.organizations {
        let organization: OrganizationId = organization.clone().try_into()?;
        if !pinned.contains(&organization) {
            pinned.push(organization);
        }
    }
    let access = JwksFixtureServer::start().await?;
    let socket = PathBuf::from(arguments.get("socket")?);
    let server = TokioServer::start_with_sign_in_and_controls(
        TokioSettings {
            legacy_organization: legacy,
            root: arguments.get("root")?.into(),
            access_policy: fixture_policy(),
            jwks_url: access.url.clone(),
            registry: fleet_sync_lease::RegistryConfig {
                lease: fleet_sync_lease::LeasePolicy {
                    lease_duration: Duration::from_secs(14_400),
                    renew_interval: Duration::from_secs(900),
                },
                authoritative_receiver: fleet_sync_protocol::ReceiverId::try_from(
                    "019f0000-0000-7000-8000-000000000010".to_owned(),
                )?,
                release_unsent_for: Duration::ZERO,
                move_needs_renewal_within: Some(Duration::from_secs(1800)),
            },
            delivery: DeliveryConfig {
                relay_peer: fleet_sync_protocol::PeerId::try_from(
                    "019f0000-0000-7000-8000-000000005240".to_owned(),
                )?,
            },
            holder_answer_timeout: Duration::from_secs(2),
            copy_ack_wait: Duration::from_secs(2),
        },
        SignInLinkSettings {
            issuer: arguments.get("issuer")?,
            audience: arguments.get("audience")?,
            admin_audience: arguments.get("admin-audience")?,
            public_origin: arguments.get("public-origin")?,
            jwks_url: arguments.get("jwks-url")?,
            socket: socket.clone(),
            service_id: arguments.get("service-id")?.try_into()?,
        },
        pinned
            .iter()
            .map(|organization| set_time(organization.clone(), fixed_time))
            .collect::<ConformanceResult<_>>()?,
    )
    .await?;
    let base_url = server.base_url().to_owned();
    println!("{}", json!({"baseUrl":base_url,"socket":socket}));
    eprintln!(
        "central launcher ready; pinned {} organizations",
        pinned.len()
    );

    let client = reqwest::Client::builder().no_proxy().build()?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    // Std stdin on its own thread: the workspace tokio has no `io-std`.
    let (line_sender, mut lines) = tokio::sync::mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lines() {
            let Ok(line) = line else { break };
            if line_sender.send(line).is_err() {
                break;
            }
        }
    });
    let mut stdin_open = true;
    loop {
        tokio::select! {
            _ = terminate.recv() => break,
            _ = tokio::signal::ctrl_c() => break,
            line = lines.recv(), if stdin_open => match line {
                Some(line) if line.trim().is_empty() => {}
                Some(line) => eprintln!("{}", answer(&client, &base_url, &line).await),
                // A closed stdin ends commands, not the server.
                None => stdin_open = false,
            },
        }
    }
    server.shutdown().await?;
    eprintln!("central launcher stopped; socket removed");
    Ok(())
}
