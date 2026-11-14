//! Refused real statements emit one closed classification through the native
//! sink; signed material and person credentials never reach captured logs.
use super::support::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::PausePoint;
use serde_json::{Value, json};
use std::{
    io::Write,
    sync::{Arc, Mutex},
    time::Duration,
};

const TARGET: &str = "collaboration_central::organization";
const VALID: &str = "019f0000-0000-7000-8000-00000000d7f0";
const HELD: &str = "019f0000-0000-7000-8000-00000000d7f1";
const UNREACHABLE: &str = "019f0000-0000-7000-8000-00000000d7f3";
const UNCONFIGURED: &str = "019f0000-0000-7000-8000-00000000d7f4";

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
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        let output = self.clone();
        tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .json()
                .with_ansi(false)
                .without_time()
                .with_writer(move || output.clone())
                .finish(),
        )
    }

    fn text(&self) -> Result<String> {
        String::from_utf8(self.0.lock().map_err(|_| "log capture poisoned")?.clone())
            .map_err(|_| "captured log was not UTF-8".into())
    }

    fn organization_events(&self) -> Result<Vec<Value>> {
        let mut found = Vec::new();
        for line in self.text()?.lines() {
            let event: Value =
                serde_json::from_str(line).map_err(|_| "captured log was not a JSON event")?;
            if event.get("target").and_then(Value::as_str) == Some(TARGET) {
                found.push(event);
            }
        }
        Ok(found)
    }

    fn refusals(&self, organization: &str) -> Result<Vec<String>> {
        self.organization_events()?
            .iter()
            .filter(|event| {
                event
                    .pointer("/fields/organization")
                    .and_then(Value::as_str)
                    == Some(organization)
            })
            .map(|event| {
                event
                    .pointer("/fields/refusal")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| "refusal classification absent".into())
            })
            .collect()
    }
}

/// Scan only by booleans, so even a leaking implementation cannot print the
/// offending log, statement, segment, digest, key ID, token or proof on failure.
#[expect(
    clippy::panic_in_result_fn,
    reason = "Secret scans fail without printing the matched material."
)]
fn assert_no_material(logs: &CapturedLogs, compact_values: &[String]) -> Result {
    let captured = logs.text()?;
    for compact in compact_values {
        assert!(
            !captured.contains(compact),
            "a compact credential reached the log"
        );
        for segment in compact.split('.').filter(|segment| segment.len() > 8) {
            assert!(
                !captured.contains(segment),
                "a signed segment reached the log"
            );
        }
        assert!(
            !captured.contains(&signed::digest(compact)),
            "a credential digest reached the log"
        );
        let kid = compact
            .split('.')
            .next()
            .and_then(|header| URL_SAFE_NO_PAD.decode(header).ok())
            .and_then(|header| serde_json::from_slice::<Value>(&header).ok())
            .and_then(|header| header.get("kid")?.as_str().map(str::to_owned))
            .filter(|kid| !kid.is_empty());
        if let Some(kid) = kid {
            assert!(
                !captured.contains(&kid),
                "a signing key identifier reached the log"
            );
        }
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Route and sanitized log assertions propagate fixture errors."
)]
#[tokio::test(flavor = "current_thread")]
async fn every_statement_profile_refusal_logs_one_closed_kind_without_signed_material() -> Result {
    // Wrong builds: a missing/duplicate/misclassified event; a valid stamp
    // logged as refused; any request material written by the real native sink.
    let logs = CapturedLogs::default();
    let _subscriber = logs.install();
    let f = Fixture::new().await?;
    let mut material = Vec::new();
    for index in 0..13 {
        let organization = format!("019f0000-0000-7000-8000-00000000d7{index:02x}");
        let case = signed::statement_refusal_case(index, &organization, signed::NOW_SECONDS)?;
        let response = f.provision(&organization, &case.statement).await?;
        assert!(
            response.body.is_empty(),
            "every statement refusal keeps the empty error body"
        );
        let refusals = logs.refusals(&organization)?;
        assert_eq!(
            response.status, case.status,
            "{}: HTTP classification",
            case.name
        );
        assert!(
            refusals.len() == 1 && refusals.first().map(String::as_str) == Some(case.refusal),
            "{}: exactly one event must name the expected closed kind",
            case.name,
        );
        // Each vector addresses its own fresh org. The foreign-org vector
        // also carries B's genuine compact, so neither namespace may be written.
        for checked_organization in [organization.as_str(), B] {
            let identities=f.rows(checked_organization,"SELECT id,organization_statement,created_at,updated_at FROM organization_identity ORDER BY id",vec![]).await?;
            let records=f.rows(checked_organization,"SELECT id,request_id,kind,origin,actor_person_id,permission_checked,target_id,created_at,updated_at FROM organization_change_records ORDER BY id",vec![]).await?;
            assert!(
                identities.is_empty() && records.is_empty(),
                "{}: the refused profile wrote an identity or record in {}",
                case.name,
                checked_organization,
            );
        }
        material.push(case.statement);
    }
    assert_eq!(
        logs.organization_events()?.len(),
        13,
        "all historical profile/refusal cases emitted once"
    );

    let valid = signed::stamp(VALID, signed::NOW_SECONDS)?;
    assert_eq!(f.provision(VALID, &valid).await?.status, 200);
    material.push(valid);
    // A real DPoP-bound person request also supplies credentials to the same
    // link. Missing floors refuse its admission without a statement refusal.
    let person = signed::Person::owner(VALID, signed::NOW_SECONDS)?;
    let body = json!({"organizationId":VALID,"workspaceId":WORKSPACE});
    let headers = person.headers(
        signed::LOOKUP,
        &signed::lookup_intent(VALID, WORKSPACE),
        &signed::fresh_request_id(signed::NOW_SECONDS)?,
        signed::NOW_SECONDS,
    )?;
    material.extend(
        headers
            .iter()
            .map(|(_, value)| value.strip_prefix("DPoP ").unwrap_or(value).to_owned()),
    );
    let response = post(
        &f.client(),
        signed::LOOKUP,
        serde_json::to_string(&body)?,
        headers,
    )
    .await?;
    assert_eq!(response.status, 200);
    assert!(
        logs.refusals(VALID)?.is_empty(),
        "valid statement control emitted a refusal"
    );
    f.close().await?;
    assert_eq!(
        logs.organization_events()?.len(),
        13,
        "the valid control adds no event"
    );
    assert_no_material(&logs, &material)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The real pre-sample hold must preserve the apply-expiry log oracle."
)]
#[tokio::test(flavor = "current_thread")]
async fn statement_expiring_after_verification_logs_one_applying_time_refusal() -> Result {
    let logs = CapturedLogs::default();
    let _subscriber = logs.install();
    let f = Fixture::new().await?;
    let statement = signed::statement_with_times(
        HELD,
        signed::NOW_SECONDS,
        signed::NOW_SECONDS - 60,
        signed::NOW_SECONDS + 1,
    )?;
    let runtime = f.runtime(HELD).await?;
    let mut held = runtime
        .pause_points()
        .hold_central(PausePoint::BeforeSignInSample);
    let client = f.client();
    let body = serde_json::to_string(&provision_body(HELD, &statement))?;
    let request = tokio::spawn(async move { post(&client, PROVISION, body, vec![]).await });
    held.until_held().await;
    assert!(
        logs.organization_events()?.is_empty(),
        "verification must succeed before the hold"
    );
    f.clock.advance(Duration::from_secs(1))?;
    held.release();
    assert_eq!(
        request.await??.status,
        422,
        "the applying sample is exactly exp"
    );
    let refusals = logs.refusals(HELD)?;
    assert!(
        refusals.len() == 1 && refusals.first().map(String::as_str) == Some("Expired"),
        "apply-time expiry must emit exactly once"
    );
    assert_eq!(logs.organization_events()?.len(), 1);
    f.close().await?;
    assert_no_material(&logs, &[statement])
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A stopped real key source must emit one sanitized unavailable classification."
)]
#[tokio::test(flavor = "current_thread")]
async fn unreachable_key_publication_logs_one_unavailable_refusal_without_statement_material()
-> Result {
    let logs = CapturedLogs::default();
    let _subscriber = logs.install();
    let mut f = Fixture::new().await?;
    let valid = signed::stamp(VALID, signed::NOW_SECONDS)?;
    assert_eq!(f.provision(VALID, &valid).await?.status, 200);
    assert!(
        logs.organization_events()?.is_empty(),
        "valid control logs no refusal"
    );

    // The host retains the original URL/cache. This replacement is only a
    // fixture teardown handle, allowing Fixture::close to shut down normally
    // after the actual configured key server has stopped.
    let configured_keys = std::mem::replace(&mut f.keys, signed::SignInKeys::start().await?);
    configured_keys.shutdown().await?;
    let statement = signed::statement_with_header(
        UNREACHABLE,
        signed::NOW_SECONDS,
        json!({
            "typ":"fleet-organization-stamp+jwt", "alg":"EdDSA", "kid":"stamp-y"
        }),
        9,
    )?;
    assert_eq!(f.provision(UNREACHABLE, &statement).await?.status, 503);
    let refusals = logs.refusals(UNREACHABLE)?;
    assert!(
        refusals.len() == 1
            && refusals
                .first()
                .is_some_and(|refusal| refusal.starts_with("VerificationUnavailable(")),
        "a stopped configured source emits one unavailable verification classification",
    );
    assert_eq!(logs.organization_events()?.len(), 1);
    assert!(
        logs.refusals(VALID)?.is_empty(),
        "valid control logs no refusal"
    );
    f.close().await?;
    assert_no_material(&logs, &[valid, statement])
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Unusable host settings must refuse and emit only the closed configuration classification."
)]
#[tokio::test(flavor = "current_thread")]
async fn unusable_signin_configuration_logs_one_invalid_configuration_without_statement_material()
-> Result {
    let logs = CapturedLogs::default();
    let _subscriber = logs.install();
    let f = Fixture::new_with_issuer("").await?;
    let statement = signed::stamp(UNCONFIGURED, signed::NOW_SECONDS)?;
    assert!(
        logs.organization_events()?.is_empty(),
        "startup is not an offered statement refusal"
    );
    assert_eq!(f.provision(UNCONFIGURED, &statement).await?.status, 503);
    let refusals = logs.refusals(UNCONFIGURED)?;
    assert!(
        refusals.len() == 1 && refusals.first().map(String::as_str) == Some("InvalidConfiguration"),
        "an offered statement under unusable configuration emits exactly one configuration refusal",
    );
    assert_eq!(logs.organization_events()?.len(), 1);
    assert_eq!(
        f.keys.fetches(),
        0,
        "invalid trust never reaches the key source"
    );
    let rows = f
        .rows(
            UNCONFIGURED,
            "SELECT COUNT(*) FROM organization_identity",
            vec![],
        )
        .await?;
    assert!(
        rows == vec![vec![number(0)?]],
        "invalid trust never stamps the organization"
    );
    f.close().await?;
    assert_no_material(&logs, &[statement])
}
