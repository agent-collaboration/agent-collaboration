use collaboration_central_core::{
    AccessCaller, AccessCheckOutcome, AccessKeyCache, AccessKeySource, AccessPolicy, AccessRefusal,
    AccessRouteClass, CentralApplication, CentralObservation, CentralPlatform, KeyFetchFailure,
    RawJwks, SignatureValidity, VerifyFailure,
};
use runtime_primitives::{Clock, SyncTime};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering},
    },
};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
const NOW: i64 = 1_792_281_600_000;
// A synthetic odd 2048-bit modulus for policy tests with controlled crypto.
const PROFILE_MODULUS: &str = "gAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQ";
const FIRST_HEADER: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6ImZpcnN0In0";
const ROTATED_HEADER: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6InJvdGF0ZWQifQ";
// Literal claims: fixture issuer, session audience, expiry 10000000000,
// app service token, common_name=fixture-client, empty subject.
const MACHINE_CLAIMS: &str = "eyJpc3MiOiJodHRwczovL2ZpeHR1cmUuY2xvdWRmbGFyZWFjY2Vzcy5jb20iLCJhdWQiOiJzZXNzaW9uIiwiZXhwIjoxMDAwMDAwMDAwMCwidHlwZSI6ImFwcCIsImNvbW1vbl9uYW1lIjoiZml4dHVyZS1jbGllbnQiLCJzdWIiOiIifQ";
const OWNER_CLAIMS: &str = "eyJpc3MiOiJodHRwczovL2ZpeHR1cmUuY2xvdWRmbGFyZWFjY2Vzcy5jb20iLCJhdWQiOiJhZG1pbiIsImV4cCI6MTAwMDAwMDAwMDAsInR5cGUiOiJhcHAiLCJlbWFpbCI6Im93bmVyQGV4YW1wbGUuaW52YWxpZCIsInN1YiI6Im93bmVyLXN1YmplY3QifQ";
const EXPIRED_CLAIMS: &str = "eyJpc3MiOiJodHRwczovL2ZpeHR1cmUuY2xvdWRmbGFyZWFjY2Vzcy5jb20iLCJhdWQiOiJzZXNzaW9uIiwiZXhwIjoxNzkyMjgxNjAwLCJ0eXBlIjoiYXBwIiwiY29tbW9uX25hbWUiOiJmaXh0dXJlLWNsaWVudCIsInN1YiI6IiJ9";

fn assertion(header: &str, claims: &str) -> String {
    format!("{header}.{claims}.c2lnbmF0dXJl")
}
fn policy() -> AccessPolicy {
    AccessPolicy {
        issuer: "https://fixture.cloudflareaccess.com".into(),
        jwks_url: "https://fixture.cloudflareaccess.com/cdn-cgi/access/certs".into(),
        session_audience: "session".into(),
        admin_audience: "admin".into(),
        owner_identity: "owner@example.invalid".into(),
    }
}
fn keys(kids: &[&str], cache_control: Option<&str>) -> RawJwks {
    RawJwks {
        body: serde_json::json!({"keys": kids.iter().map(|kid| serde_json::json!({
            "kid":kid, "kty":"RSA", "n":PROFILE_MODULUS, "e":"AQAB", "alg":"RS256", "use":"sig"
        })).collect::<Vec<_>>()})
        .to_string()
        .into_bytes(),
        cache_control: cache_control.map(str::to_owned),
    }
}

#[derive(Clone)]
struct FixturePlatform(Arc<FixtureState>);
struct FixtureState {
    now: AtomicI64,
    responses: Mutex<VecDeque<Result<RawJwks, KeyFetchFailure>>>,
    fetches: AtomicUsize,
    verified_keys: Mutex<Vec<String>>,
    signature_valid: AtomicBool,
    observations: Mutex<Vec<CentralObservation>>,
}
impl FixturePlatform {
    fn new(responses: Vec<Result<RawJwks, KeyFetchFailure>>) -> Self {
        Self(Arc::new(FixtureState {
            now: AtomicI64::new(NOW),
            responses: Mutex::new(responses.into()),
            fetches: AtomicUsize::new(0),
            verified_keys: Mutex::new(Vec::new()),
            signature_valid: AtomicBool::new(true),
            observations: Mutex::new(Vec::new()),
        }))
    }
    fn after_seconds(&self, seconds: i64) {
        self.0.now.store(NOW + seconds * 1000, Ordering::SeqCst);
    }
}
impl Clock for FixturePlatform {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.now.load(Ordering::SeqCst))
    }
}
impl CentralPlatform for FixturePlatform {
    type Host = collaboration_central_tokio::TokioStorageHost<
        collaboration_central_core::CoreHost<collaboration_central_tokio::TokioObjectIo>,
    >;
    // These legacy Access policy fixtures never exercise issuer cryptography.
    async fn verify_ed25519(
        &self,
        _: &[u8],
        _: &[u8],
        _: &[u8],
    ) -> Result<SignatureValidity, VerifyFailure> {
        Err(VerifyFailure::Crypto)
    }
    fn clock(&self) -> &dyn Clock {
        self
    }
    fn ids(&self) -> &dyn runtime_primitives::IdSource {
        &runtime_primitives::UuidV7Ids
    }
    // These policy fixtures exercise no object transport or socket upgrade.
    // The native HTTP suite covers those real capabilities separately.
    async fn transport_object(
        &self,
        _: collaboration_central_core::SerializedObjectCall,
    ) -> Result<Vec<u8>, durable_steps::CallFailure> {
        Err(durable_steps::CallFailure::NotSent {
            reason: durable_steps::NotSentReason::Overloaded,
        })
    }
    async fn upgrade_session(
        &self,
        _: axum::http::request::Parts,
        _: fleet_sync_session::ConnectionBinding,
        _: collaboration_central_core::AdmissionOrigin,
    ) -> Result<axum::http::Response<axum::body::Body>, collaboration_central_core::UpgradeFailure>
    {
        Err(collaboration_central_core::UpgradeFailure::Refused)
    }
    async fn fetch_access_keys(&self) -> Result<RawJwks, KeyFetchFailure> {
        self.0.fetches.fetch_add(1, Ordering::SeqCst);
        self.0
            .responses
            .lock()
            .map_err(|_| KeyFetchFailure::Unreachable)?
            .pop_front()
            .unwrap_or(Err(KeyFetchFailure::Unreachable))
    }
    async fn verify_rs256(
        &self,
        key: &collaboration_central_core::AccessJwk,
        signed: &[u8],
        signature: &[u8],
    ) -> Result<SignatureValidity, VerifyFailure> {
        if signed.is_empty() || signature != b"signature" {
            return Err(VerifyFailure::Crypto);
        }
        self.0
            .verified_keys
            .lock()
            .map_err(|_| VerifyFailure::Crypto)?
            .push(key.kid.clone());
        Ok(if self.0.signature_valid.load(Ordering::SeqCst) {
            SignatureValidity::Valid
        } else {
            SignatureValidity::Invalid
        })
    }
    fn observe(&self, observation: CentralObservation) {
        self.0
            .observations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(observation);
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn key_expiry_honors_max_age_with_the_default_and_one_hour_cap() -> TestResult {
    for (header, lifetime) in [
        (None, 3600),
        (Some("public, max-age=2"), 2),
        (Some("max-age=999999"), 3600),
        (Some("max-age=invalid"), 3600),
    ] {
        let platform = FixturePlatform::new(vec![
            Ok(keys(&["first"], header)),
            Ok(keys(&["first"], header)),
        ]);
        let application = CentralApplication::new(
            platform.clone(),
            policy(),
            Arc::new(AccessKeyCache::default()),
        )?;
        let token = assertion(FIRST_HEADER, MACHINE_CLAIMS);

        assert!(
            matches!(application.verify_access(Some(&token), "/v1/session").await?, AccessCaller::Machine {client_id} if client_id.as_str()=="fixture-client")
        );
        platform.after_seconds(lifetime - 1);
        application
            .verify_access(Some(&token), "/v1/session")
            .await?;
        assert_eq!(
            platform.0.fetches.load(Ordering::SeqCst),
            1,
            "header {header:?}"
        );
        platform.after_seconds(lifetime);
        application
            .verify_access(Some(&token), "/v1/session")
            .await?;

        assert_eq!(
            platform.0.fetches.load(Ordering::SeqCst),
            2,
            "header {header:?}"
        );
        let observations = platform
            .0
            .observations
            .lock()
            .map_err(|_| "observation lock poisoned")?;
        assert_eq!(
            *observations,
            vec![
                CentralObservation::AccessCheck {
                    route_class: AccessRouteClass::Session,
                    outcome: AccessCheckOutcome::AcceptedMachine,
                    key_source: Some(AccessKeySource::Fetched)
                },
                CentralObservation::AccessCheck {
                    route_class: AccessRouteClass::Session,
                    outcome: AccessCheckOutcome::AcceptedMachine,
                    key_source: Some(AccessKeySource::Cached)
                },
                CentralObservation::AccessCheck {
                    route_class: AccessRouteClass::Session,
                    outcome: AccessCheckOutcome::AcceptedMachine,
                    key_source: Some(AccessKeySource::Fetched)
                },
            ]
        );
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn zero_max_age_does_not_reuse_keys_at_the_same_clock_value() -> TestResult {
    let platform = FixturePlatform::new(vec![
        Ok(keys(&["first"], Some("max-age=0"))),
        Ok(keys(&["first"], None)),
    ]);
    let application = CentralApplication::new(
        platform.clone(),
        policy(),
        Arc::new(AccessKeyCache::default()),
    )?;
    let token = assertion(FIRST_HEADER, MACHINE_CLAIMS);
    application
        .verify_access(Some(&token), "/v1/session")
        .await?;
    application
        .verify_access(Some(&token), "/v1/session")
        .await?;
    assert_eq!(platform.0.fetches.load(Ordering::SeqCst), 2);
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn an_unknown_key_refreshes_before_verifying_the_rotated_assertion() -> TestResult {
    let platform = FixturePlatform::new(vec![
        Ok(keys(&["first"], None)),
        Ok(keys(&["rotated"], None)),
    ]);
    let application = CentralApplication::new(
        platform.clone(),
        policy(),
        Arc::new(AccessKeyCache::default()),
    )?;
    application
        .verify_access(
            Some(&assertion(FIRST_HEADER, MACHINE_CLAIMS)),
            "/v1/session",
        )
        .await?;

    let caller = application
        .verify_access(
            Some(&assertion(ROTATED_HEADER, MACHINE_CLAIMS)),
            "/v1/session",
        )
        .await?;

    assert!(matches!(caller, AccessCaller::Machine { .. }));
    assert_eq!(platform.0.fetches.load(Ordering::SeqCst), 2);
    assert_eq!(
        *platform
            .0
            .verified_keys
            .lock()
            .map_err(|_| "signature lock poisoned")?,
        vec!["first", "rotated"]
    );
    assert_eq!(
        platform
            .0
            .observations
            .lock()
            .map_err(|_| "observation lock poisoned")?
            .last(),
        Some(&CentralObservation::AccessCheck {
            route_class: AccessRouteClass::Session,
            outcome: AccessCheckOutcome::AcceptedMachine,
            key_source: Some(AccessKeySource::Refetched)
        })
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn missing_malformed_and_expired_assertions_are_refused_without_capabilities() -> TestResult {
    let expired = assertion(FIRST_HEADER, EXPIRED_CLAIMS);
    for (token, expected) in [
        (None, AccessRefusal::MissingAssertion),
        (Some("malformed"), AccessRefusal::MalformedAssertion),
        (Some(expired.as_str()), AccessRefusal::Expired),
    ] {
        let platform = FixturePlatform::new(Vec::new());
        let application = CentralApplication::new(
            platform.clone(),
            policy(),
            Arc::new(AccessKeyCache::default()),
        )?;
        assert_eq!(
            application.verify_access(token, "/v1/session").await,
            Err(expected.clone())
        );
        assert_eq!(platform.0.fetches.load(Ordering::SeqCst), 0);
        assert!(
            platform
                .0
                .verified_keys
                .lock()
                .map_err(|_| "signature lock poisoned")?
                .is_empty()
        );
        assert_eq!(
            *platform
                .0
                .observations
                .lock()
                .map_err(|_| "observation lock poisoned")?,
            vec![CentralObservation::AccessCheck {
                route_class: AccessRouteClass::Session,
                outcome: AccessCheckOutcome::Refused(expected),
                key_source: None
            }]
        );
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn bad_signature_and_wrong_owner_never_release_an_authorized_caller() -> TestResult {
    let platform = FixturePlatform::new(vec![Ok(keys(&["first"], None))]);
    let application = CentralApplication::new(
        platform.clone(),
        policy(),
        Arc::new(AccessKeyCache::default()),
    )?;
    platform.0.signature_valid.store(false, Ordering::SeqCst);
    assert_eq!(
        application
            .verify_access(
                Some(&assertion(FIRST_HEADER, MACHINE_CLAIMS)),
                "/v1/session"
            )
            .await,
        Err(AccessRefusal::BadSignature)
    );
    platform.0.signature_valid.store(true, Ordering::SeqCst);
    let mut other_owner = policy();
    other_owner.owner_identity = "different@example.invalid".into();
    let owner_application = CentralApplication::new(
        platform.clone(),
        other_owner,
        Arc::new(AccessKeyCache::default()),
    )?;
    platform
        .0
        .responses
        .lock()
        .map_err(|_| "response lock poisoned")?
        .push_back(Ok(keys(&["first"], None)));
    assert_eq!(
        owner_application
            .verify_access(
                Some(&assertion(FIRST_HEADER, OWNER_CLAIMS)),
                "/admin/v1/operations"
            )
            .await,
        Err(AccessRefusal::WrongOwner)
    );
    assert_eq!(
        application
            .verify_access(
                Some(&assertion(FIRST_HEADER, OWNER_CLAIMS)),
                "/admin/v1/operations"
            )
            .await?,
        AccessCaller::Owner {
            email: "owner@example.invalid".into()
        }
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn fetch_parse_and_key_selection_failures_refuse_before_crypto() -> TestResult {
    for (responses, expected, fetches) in [
        (
            vec![Err(KeyFetchFailure::Unreachable)],
            AccessRefusal::KeySourceUnavailable,
            1,
        ),
        (
            vec![Ok(RawJwks {
                body: b"{".to_vec(),
                cache_control: None,
            })],
            AccessRefusal::KeySourceUnavailable,
            1,
        ),
        (
            vec![Ok(keys(&["first", "first"], None))],
            AccessRefusal::InvalidKey,
            1,
        ),
        (
            vec![Ok(keys(&[], None)), Ok(keys(&[], None))],
            AccessRefusal::UnknownKey,
            2,
        ),
    ] {
        let platform = FixturePlatform::new(responses);
        let application = CentralApplication::new(
            platform.clone(),
            policy(),
            Arc::new(AccessKeyCache::default()),
        )?;
        assert_eq!(
            application
                .verify_access(
                    Some(&assertion(FIRST_HEADER, MACHINE_CLAIMS)),
                    "/v1/session"
                )
                .await,
            Err(expected)
        );
        assert_eq!(platform.0.fetches.load(Ordering::SeqCst), fetches);
        assert!(
            platform
                .0
                .verified_keys
                .lock()
                .map_err(|_| "signature lock poisoned")?
                .is_empty()
        );
    }
    Ok(())
}
