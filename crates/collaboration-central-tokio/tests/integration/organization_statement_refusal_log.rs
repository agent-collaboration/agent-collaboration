//! A refused organization statement leaves exactly one log event naming its
//! closed refusal kind and the Fleet's organization, and no log line carries
//! statement material: not its bytes, segments, digest or key ID.
#![cfg(feature = "conformance")]
use crate::fleet_clock::FleetClock;
use crate::organization_admin_link::settings;
use crate::organization_provisioning_link::{PROVISION, control, provision_body};
use crate::organization_stamps::{
    self as stamps, ADMIN_AUDIENCE, AUDIENCE, ISSUER, LIFETIME_SECONDS, NOW_SECONDS, PUBLIC_ORIGIN,
    SignInKeys, link_post,
};
use crate::statement_refusals::{StatementRefusal, further_refusals, profile_refusals};
use collaboration_central_conformance::{
    ConformanceResult, DispatchCallSelector, DispatchKey, JwksFixtureServer, RuntimeControl,
};
use collaboration_central_core::ObjectAddress;
use collaboration_central_tokio::{SignInLinkSettings, TokioServer};
use serde_json::Value;
use std::{
    io::Write,
    path::Path,
    sync::{Arc, Mutex},
};

/// Provisioned by the valid control, which must log no refusal.
const VALID: &str = "019f0000-0000-7000-8000-00000000d7f0";
/// Released into its applying transaction at its own exp.
const HELD: &str = "019f0000-0000-7000-8000-00000000d7f1";
/// Names the statement used by the another-organization case.
const OTHER: &str = "019f0000-0000-7000-8000-00000000d7f2";

#[derive(Clone, Default)]
struct CapturedLogs(Arc<Mutex<Vec<u8>>>);
impl Write for CapturedLogs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("log capture poisoned"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl CapturedLogs {
    fn text(&self) -> ConformanceResult<String> {
        Ok(String::from_utf8(
            self.0.lock().map_err(|_| "log capture poisoned")?.clone(),
        )?)
    }
    /// The refusal kinds logged for `organization`, in order.
    fn refusals(&self, organization: &str) -> ConformanceResult<Vec<String>> {
        let mut found = Vec::new();
        for line in self.text()?.lines() {
            let event: Value = serde_json::from_str(line)?;
            if event.pointer("/target").and_then(Value::as_str)
                == Some("collaboration_central::organization")
                && event
                    .pointer("/fields/organization")
                    .and_then(Value::as_str)
                    == Some(organization)
            {
                found.push(
                    event
                        .pointer("/fields/refusal")
                        .and_then(Value::as_str)
                        .ok_or("refusal field")?
                        .to_owned(),
                );
            }
        }
        Ok(found)
    }
}

fn case_organization(index: usize) -> String {
    format!("019f0000-0000-7000-8000-00000000d7{index:02x}")
}

async fn start(
    root: &Path,
    access: &JwksFixtureServer,
    keys: &SignInKeys,
    clock: &FleetClock,
) -> ConformanceResult<(TokioServer, std::path::PathBuf)> {
    let socket = root.join("sign-in-link.sock");
    let server = TokioServer::start_with_sign_in_controls_and_fleet_clock(
        settings(root, &access.url)?,
        SignInLinkSettings {
            issuer: ISSUER.into(),
            audience: AUDIENCE.into(),
            admin_audience: ADMIN_AUDIENCE.into(),
            public_origin: PUBLIC_ORIGIN.into(),
            jwks_url: keys.url.clone(),
            socket: socket.clone(),
            service_id: "019f0000-0000-7000-8000-00000000e501"
                .to_owned()
                .try_into()?,
        },
        Vec::new(),
        clock.shared(),
    )
    .await?;
    Ok((server, socket))
}

/// Hold a verified provisioning before its transaction, move the Fleet clock
/// to `release_millis`, then release it.
async fn held_until(
    server: &TokioServer,
    socket: &Path,
    clock: &FleetClock,
    statement: String,
    release_millis: i64,
) -> ConformanceResult<u16> {
    let base = server.base_url().to_owned();
    let key = DispatchKey::new(
        ObjectAddress::Fleet {
            organization: HELD.to_owned().try_into()?,
        },
        DispatchCallSelector::OrganizationProvisioning,
    );
    control(
        &base,
        HELD,
        RuntimeControl::HoldNextDispatch { key: key.clone() },
    )
    .await?;
    let held_socket = socket.to_owned();
    let held_call = tokio::spawn(async move {
        link_post(&held_socket, PROVISION, &provision_body(HELD, &statement))
            .await
            .map_err(|error| error.to_string())
    });
    control(
        &base,
        HELD,
        RuntimeControl::AwaitHeldDispatch { key: key.clone() },
    )
    .await?;
    clock.set(release_millis);
    control(&base, HELD, RuntimeControl::ResumeDispatch { key }).await?;
    Ok(held_call.await??.0)
}

#[expect(
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    reason = "One run keeps every answer, event count and the material scan together while fixture errors propagate."
)]
#[tokio::test]
async fn each_statement_refusal_logs_its_kind_once_and_no_statement_material()
-> ConformanceResult<()> {
    // Wrong builds caught: a refusal kind that is dropped or logged twice; a
    // refusal reported under the wrong kind; a log line carrying statement
    // bytes, a segment, its digest or its key ID; a valid provisioning logged
    // as refused; an apply-time expiry left unlogged.
    let logs = CapturedLogs::default();
    let output = logs.clone();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .json()
            .with_ansi(false)
            .without_time()
            .with_writer(move || output.clone())
            .finish(),
    )?;
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let clock = FleetClock::at(NOW_SECONDS * 1000);
    let (server, socket) = start(root.path(), &access, &keys, &clock).await?;

    // Each case on its own never-named organization, so its events are its own.
    let mut statements = Vec::new();
    let mut index = 0;
    let mut planned: Vec<(String, StatementRefusal, u16)> = Vec::new();
    for _ in 0..7 {
        let organization = case_organization(index);
        index += 1;
        let refusal = profile_refusals(&organization)?.remove(planned.len());
        planned.push((organization, refusal, 422));
    }
    for further in 0..6 {
        let organization = case_organization(index);
        index += 1;
        let refusal = further_refusals(&organization, OTHER)?.remove(further);
        let status = if refusal.refusal == "OrganizationMismatch" {
            403
        } else {
            422
        };
        planned.push((organization, refusal, status));
    }
    let mut observed = Vec::new();
    for (organization, refusal, expected_status) in planned {
        let answer = link_post(
            &socket,
            PROVISION,
            &provision_body(&organization, &refusal.statement),
        )
        .await?;
        // The event is written before the answer leaves the Fleet.
        observed.push((
            refusal.case,
            answer.0,
            expected_status,
            logs.refusals(&organization)?,
            refusal.refusal,
        ));
        statements.push(refusal.statement);
    }

    let valid = stamps::stamp(VALID)?;
    let valid_answer = link_post(&socket, PROVISION, &provision_body(VALID, &valid)).await?;
    let valid_refusals = logs.refusals(VALID)?;
    statements.push(valid);

    let held = stamps::stamp(HELD)?;
    let held_answer = held_until(
        &server,
        &socket,
        &clock,
        held.clone(),
        (NOW_SECONDS - 60 + LIFETIME_SECONDS) * 1000,
    )
    .await?;
    let held_refusals = logs.refusals(HELD)?;
    statements.push(held);
    server.shutdown().await?;

    for (case, status, expected_status, refusals, expected) in &observed {
        assert_eq!(status, expected_status, "{case}: answer");
        assert_eq!(
            refusals,
            &vec![(*expected).to_owned()],
            "{case}: exactly one event naming its kind"
        );
    }
    assert_eq!(
        observed.len(),
        13,
        "every refusal kind reachable by a statement"
    );
    assert_eq!(valid_answer.0, 200, "the valid control provisions");
    assert!(
        valid_refusals.is_empty(),
        "a valid provisioning logs no refusal"
    );
    assert_eq!(held_answer, 422, "released at its exp into the transaction");
    assert_eq!(
        held_refusals,
        vec!["Expired".to_owned()],
        "apply-time expiry logged once"
    );

    let text = logs.text()?;
    for statement in &statements {
        let mut material: Vec<String> = statement
            .split('.')
            .filter(|segment| segment.len() > 8)
            .map(str::to_owned)
            .collect();
        material.push(stamps::digest(statement));
        for needle in material {
            assert!(
                !text.contains(&needle),
                "statement material reached the log"
            );
        }
    }
    for kid in ["stamp-a", "stamp-b", "stamp-z"] {
        assert!(!text.contains(kid), "a key ID reached the log");
    }
    Ok(())
}
