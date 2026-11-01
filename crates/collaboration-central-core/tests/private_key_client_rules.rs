//! RFC7523/RFC7662 request formation, with controlled IDs and clocks.
#![cfg(test)]
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::{
    ClientAssertionLifetime, ClientAssertionRefusal, Ed25519ClientSigner,
    PersonIntrospectionClient, PersonSessionConfirmer, PrivateKeyJwtClient, ServiceClientId,
    ServiceForm, ServiceFormFailure, ServiceFormTransport,
    identity_contract::ResourceUri,
    organization_authority::{PersonSessionConfirmation, SignInUnavailable},
};
use runtime_primitives::{Clock, IdSource, SyncTime};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};

struct FixedClock(i64);
impl Clock for FixedClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0)
    }
}
#[derive(Default)]
struct ScriptedIds(AtomicU64);
impl IdSource for ScriptedIds {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::from_u128(
            0x019f0000000070008000000000000000 + u128::from(self.0.fetch_add(1, Ordering::SeqCst)),
        )
    }
}
struct InvalidIds;
impl IdSource for InvalidIds {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::nil()
    }
}
struct SignerDouble {
    kid: &'static str,
    calls: Arc<AtomicUsize>,
    answer: Result<Vec<u8>, ClientAssertionRefusal>,
}
impl Ed25519ClientSigner for SignerDouble {
    fn key_id(&self) -> &str {
        self.kid
    }
    async fn sign_client_assertion(&self, _: &[u8]) -> Result<Vec<u8>, ClientAssertionRefusal> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.answer.clone()
    }
}
fn signer() -> SignerDouble {
    SignerDouble {
        kid: "public-fixture-key",
        calls: Arc::default(),
        answer: Ok(vec![1; 64]),
    }
}
fn uri(value: &str) -> ResourceUri {
    ResourceUri::try_from(value.to_owned()).unwrap()
}
fn client(signer: SignerDouble) -> PrivateKeyJwtClient<SignerDouble> {
    PrivateKeyJwtClient::new(
        ServiceClientId::try_from("central-client".to_owned()).unwrap(),
        signer,
        Arc::new(FixedClock(2_208_988_800_999)),
        Arc::new(ScriptedIds::default()),
        ClientAssertionLifetime::from_seconds(60).unwrap(),
    )
    .unwrap()
}
fn fields(form: &ServiceForm) -> BTreeMap<String, String> {
    url::form_urlencoded::parse(form.as_str().as_bytes())
        .into_owned()
        .collect()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Claims {
    iss: String,
    sub: String,
    aud: String,
    iat: i64,
    exp: i64,
    jti: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    alg: String,
    typ: String,
    kid: String,
}
fn claims(fields: &BTreeMap<String, String>) -> Claims {
    let segments: Vec<_> = fields["client_assertion"].split('.').collect();
    assert_eq!(segments.len(), 3);
    assert!(segments.iter().all(|segment| !segment.contains('=')));
    let header: Header =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(segments[0]).unwrap()).unwrap();
    assert_eq!(
        (
            header.alg.as_str(),
            header.typ.as_str(),
            header.kid.as_str()
        ),
        ("EdDSA", "JWT", "public-fixture-key")
    );
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(segments[1]).unwrap()).unwrap()
}

// Signing is a test double here. These catch wrong claims, replayed assertions,
// clock/ID fallbacks and leaking the person bearer into Authorization instead
// of the token field. Actual host signing/transport proof is a separate test.
#[tokio::test(start_paused = true)]
async fn introspection_uses_fresh_endpoint_bound_client_authentication() {
    let signer = signer();
    let calls = signer.calls.clone();
    let client = client(signer);
    let endpoint = uri("https://issuer.example.invalid/api/auth/oauth2/introspect");
    let person = "fixture+person/token.with=reserved&characters";
    let first = fields(
        &client
            .authenticate_at(&endpoint)
            .await
            .unwrap()
            .introspection_form(person),
    );
    let second = fields(
        &client
            .authenticate_at(&endpoint)
            .await
            .unwrap()
            .introspection_form(person),
    );
    assert_eq!(first.len(), 4);
    assert_eq!(first["token"], person);
    assert_eq!(first["client_id"], "central-client");
    assert_eq!(
        first["client_assertion_type"],
        "urn:ietf:params:oauth:client-assertion-type:jwt-bearer"
    );
    let one = claims(&first);
    let two = claims(&second);
    assert_eq!(
        (one.iss.as_str(), one.sub.as_str(), one.aud.as_str()),
        ("central-client", "central-client", endpoint.as_str())
    );
    assert_eq!((one.iat, one.exp), (2_208_988_800, 2_208_988_860));
    assert_ne!(one.jti, two.jti);
    assert_eq!(
        durable_steps::Uuid::parse_str(&one.jti)
            .unwrap()
            .get_version_num(),
        7
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let token_endpoint = uri("https://issuer.example.invalid/api/auth/oauth2/token");
    let other = fields(
        &client
            .authenticate_at(&token_endpoint)
            .await
            .unwrap()
            .introspection_form(person),
    );
    assert_eq!(claims(&other).aud, token_endpoint.as_str());
    assert_ne!(claims(&other).jti, two.jti);
}

#[tokio::test(start_paused = true)]
async fn assertion_failures_never_invent_a_clock_id_or_signature() {
    let identity = || ServiceClientId::try_from("central-client".to_owned()).unwrap();
    let endpoint = uri("https://issuer.example.invalid/introspect");
    let invalid_clock = PrivateKeyJwtClient::new(
        identity(),
        signer(),
        Arc::new(FixedClock(-1)),
        Arc::new(ScriptedIds::default()),
        ClientAssertionLifetime::from_seconds(60).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        invalid_clock.authenticate_at(&endpoint).await,
        Err(ClientAssertionRefusal::InvalidClock)
    ));
    let invalid_ids = PrivateKeyJwtClient::new(
        identity(),
        signer(),
        Arc::new(FixedClock(1_000)),
        Arc::new(InvalidIds),
        ClientAssertionLifetime::from_seconds(60).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        invalid_ids.authenticate_at(&endpoint).await,
        Err(ClientAssertionRefusal::InvalidId)
    ));
    for answer in [
        Err(ClientAssertionRefusal::Signing),
        Ok(vec![0; 63]),
        Ok(vec![0; 65]),
    ] {
        let mut signer = signer();
        signer.answer = answer;
        assert!(matches!(
            client(signer).authenticate_at(&endpoint).await,
            Err(ClientAssertionRefusal::Signing)
        ));
    }
}

#[test]
fn assertion_configuration_refuses_empty_key_ids_and_excess_lifetime() {
    assert!(ClientAssertionLifetime::from_seconds(0).is_err());
    assert!(ClientAssertionLifetime::from_seconds(301).is_err());
    assert!(ClientAssertionLifetime::from_seconds(300).is_ok());
    let mut signer = signer();
    signer.kid = "";
    assert!(matches!(
        PrivateKeyJwtClient::new(
            ServiceClientId::try_from("central-client".to_owned()).unwrap(),
            signer,
            Arc::new(FixedClock(0)),
            Arc::new(ScriptedIds::default()),
            ClientAssertionLifetime::from_seconds(60).unwrap()
        ),
        Err(ClientAssertionRefusal::InvalidConfiguration)
    ));
}

struct RecordingTransport {
    calls: Arc<Mutex<Vec<(String, String)>>>,
    answer: Result<Vec<u8>, ServiceFormFailure>,
}
impl ServiceFormTransport for RecordingTransport {
    async fn post_service_form(
        &self,
        endpoint: &ResourceUri,
        form: &ServiceForm,
    ) -> Result<Vec<u8>, ServiceFormFailure> {
        self.calls
            .lock()
            .unwrap()
            .push((endpoint.as_str().into(), form.as_str().into()));
        self.answer.clone()
    }
}
#[tokio::test(start_paused = true)]
async fn confirmation_authenticates_each_call_and_decodes_the_actual_k5_shape() {
    let producer: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/person-identity/K5.fixtures.json")).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let endpoint = uri("https://issuer.example.invalid/api/auth/oauth2/introspect");
    let confirmer = PersonIntrospectionClient::new(
        client(signer()),
        RecordingTransport {
            calls: calls.clone(),
            answer: Ok(serde_json::to_vec(&producer["examples"]["active"]).unwrap()),
        },
        endpoint.clone(),
    );
    for _ in 0..2 {
        assert!(matches!(
            confirmer
                .confirm_person_session("fixture-person-proof")
                .await
                .unwrap(),
            PersonSessionConfirmation::Active { .. }
        ));
    }
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|(target, _)| target == endpoint.as_str()));
    let first: BTreeMap<_, _> = url::form_urlencoded::parse(calls[0].1.as_bytes())
        .into_owned()
        .collect();
    let second: BTreeMap<_, _> = url::form_urlencoded::parse(calls[1].1.as_bytes())
        .into_owned()
        .collect();
    assert_eq!(first["token"], "fixture-person-proof");
    assert_ne!(claims(&first).jti, claims(&second).jti);
}

#[tokio::test(start_paused = true)]
async fn confirmation_refuses_unavailable_or_malformed_replies_without_guessing_inactive() {
    for answer in [
        Err(ServiceFormFailure::Unreachable),
        Err(ServiceFormFailure::Refused { status: 401 }),
        Ok(b"{}".to_vec()),
    ] {
        let confirmer = PersonIntrospectionClient::new(
            client(signer()),
            RecordingTransport {
                calls: Arc::default(),
                answer,
            },
            uri("https://issuer.example.invalid/introspect"),
        );
        assert_eq!(
            confirmer
                .confirm_person_session("fixture-person-proof")
                .await,
            Err(SignInUnavailable::ConfirmationUnreachable)
        );
    }
}
