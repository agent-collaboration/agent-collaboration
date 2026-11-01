//! Unit checks of the portable async helper using Tokio only as its executor.
//! The sign-in capability is a contract fixture, not real C7 or host integration.
#![expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail async policy regressions while fixture errors propagate."
)]

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::{
    SignatureValidity,
    organization_authority::{
        PersonConfirmationRefusal, PersonConfirmationWindow, PersonSessionConfirmation,
        PersonSignInPort, PersonSignatureInput, PersonTokenAudience, PersonTokenPolicy,
        PersonTokenRefusal, SignInUnavailable, verify_and_confirm_person,
    },
};
use runtime_primitives::{Clock, SyncTime};
use serde_json::json;
use std::{
    future::{Future, ready},
    sync::{
        Mutex,
        atomic::{AtomicI64, AtomicUsize, Ordering},
    },
};
use tokio::sync::oneshot;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

const PERSON: &str = "019a3dce-2000-7000-8000-000000000001";
const ORGANIZATION: &str = "019a3dce-2000-7000-8000-000000000002";
const ADMIN: &str = "https://central.example.invalid/v1/admin";
const NOW_MS: i64 = 1_800_000_000_000;

struct ControlledClock(AtomicI64);

impl ControlledClock {
    fn new() -> Self {
        Self(AtomicI64::new(NOW_MS))
    }

    fn set_millis(&self, millis: i64) {
        self.0.store(millis, Ordering::SeqCst);
    }
}

impl Clock for ControlledClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}

fn policy() -> Result<PersonTokenPolicy, PersonTokenRefusal> {
    PersonTokenPolicy::new(
        "https://signin.example.invalid".into(),
        ADMIN.into(),
        "https://central.example.invalid/v1/operator".into(),
        "sign-in-app".into(),
    )
}

fn bearer() -> Result<String, serde_json::Error> {
    let header = json!({"typ":"at+jwt","alg":"EdDSA","kid":"issuer-key"});
    let claims = json!({
        "iss":"https://signin.example.invalid", "sub":PERSON, "aud":ADMIN,
        "exp":1_800_000_300, "iat":1_800_000_000, "scope":"central:admin",
        "client_id":"sign-in-app", "azp":"sign-in-app", "sid":"issuer-session",
        "jti":"issuer-token", "organization_id":ORGANIZATION,
    });
    Ok(format!(
        "{}.{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header)?),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims)?),
        URL_SAFE_NO_PAD.encode([1_u8, 2, 3]),
    ))
}

fn active_confirmation() -> Result<PersonSessionConfirmation, SignInUnavailable> {
    Ok(PersonSessionConfirmation::Active {
        person_id: PERSON
            .to_owned()
            .try_into()
            .map_err(|_| SignInUnavailable::ConfirmationUnreachable)?,
        session_ref: "issuer-session"
            .to_owned()
            .try_into()
            .map_err(|_| SignInUnavailable::ConfirmationUnreachable)?,
        audience: ADMIN.into(),
        organization_id: ORGANIZATION
            .to_owned()
            .try_into()
            .map_err(|_| SignInUnavailable::ConfirmationUnreachable)?,
        expires_at: serde_json::from_value(json!(1_800_000_300))
            .map_err(|_| SignInUnavailable::ConfirmationUnreachable)?,
    })
}

struct ChangingConfirmationPort {
    signature: SignatureValidity,
    confirmations: AtomicUsize,
    expected_bearer: String,
}

impl PersonSignInPort for ChangingConfirmationPort {
    fn verify_person_signature<'request>(
        &'request self,
        _input: PersonSignatureInput<'request>,
    ) -> impl Future<Output = Result<SignatureValidity, SignInUnavailable>> + Send + 'request {
        ready(Ok(self.signature))
    }

    fn confirm_person_session<'request>(
        &'request self,
        bearer_token: &'request str,
    ) -> impl Future<Output = Result<PersonSessionConfirmation, SignInUnavailable>> + Send + 'request
    {
        ready(if bearer_token != self.expected_bearer {
            Ok(PersonSessionConfirmation::Inactive {})
        } else if self.confirmations.fetch_add(1, Ordering::SeqCst) == 0 {
            active_confirmation()
        } else {
            Ok(PersonSessionConfirmation::Inactive {})
        })
    }
}

#[tokio::test]
async fn organization_authority_each_invocation_observes_its_own_confirmation() -> TestResult {
    let bearer_token = bearer()?;
    let sign_in = ChangingConfirmationPort {
        signature: SignatureValidity::Valid,
        confirmations: AtomicUsize::new(0),
        expected_bearer: bearer_token.clone(),
    };
    let clock = ControlledClock::new();
    let policy = policy()?;
    let first = verify_and_confirm_person(
        &sign_in,
        &clock,
        &policy,
        PersonTokenAudience::Admin,
        bearer_token.clone(),
    )
    .await?;
    assert_eq!(first.token().person_id().as_str(), PERSON);
    let second = verify_and_confirm_person(
        &sign_in,
        &clock,
        &policy,
        PersonTokenAudience::Admin,
        bearer_token,
    )
    .await;
    assert!(matches!(second, Err(PersonConfirmationRefusal::Inactive)));
    Ok(())
}

#[tokio::test]
async fn organization_authority_invalid_signature_stops_before_session_confirmation() -> TestResult
{
    let bearer_token = bearer()?;
    let sign_in = ChangingConfirmationPort {
        signature: SignatureValidity::Invalid,
        confirmations: AtomicUsize::new(0),
        expected_bearer: bearer_token.clone(),
    };
    let answer = verify_and_confirm_person(
        &sign_in,
        &ControlledClock::new(),
        &policy()?,
        PersonTokenAudience::Admin,
        bearer_token,
    )
    .await;
    assert!(matches!(
        answer,
        Err(PersonConfirmationRefusal::InvalidToken(
            PersonTokenRefusal::BadSignature
        ))
    ));
    assert_eq!(sign_in.confirmations.load(Ordering::SeqCst), 0);
    Ok(())
}

struct HeldConfirmationPort {
    entered: Mutex<Option<oneshot::Sender<()>>>,
    release: Mutex<Option<oneshot::Receiver<()>>>,
    expected_bearer: String,
}

impl PersonSignInPort for HeldConfirmationPort {
    fn verify_person_signature<'request>(
        &'request self,
        _input: PersonSignatureInput<'request>,
    ) -> impl Future<Output = Result<SignatureValidity, SignInUnavailable>> + Send + 'request {
        ready(Ok(SignatureValidity::Valid))
    }

    fn confirm_person_session<'request>(
        &'request self,
        bearer_token: &'request str,
    ) -> impl Future<Output = Result<PersonSessionConfirmation, SignInUnavailable>> + Send + 'request
    {
        async move {
            if bearer_token != self.expected_bearer {
                return Ok(PersonSessionConfirmation::Inactive {});
            }
            let entered = {
                self.entered
                    .lock()
                    .map_err(|_| SignInUnavailable::ConfirmationUnreachable)?
                    .take()
                    .ok_or(SignInUnavailable::ConfirmationUnreachable)?
            };
            let release = {
                self.release
                    .lock()
                    .map_err(|_| SignInUnavailable::ConfirmationUnreachable)?
                    .take()
                    .ok_or(SignInUnavailable::ConfirmationUnreachable)?
            };
            entered
                .send(())
                .map_err(|_| SignInUnavailable::ConfirmationUnreachable)?;
            release
                .await
                .map_err(|_| SignInUnavailable::ConfirmationUnreachable)?;
            active_confirmation()
        }
    }
}

#[tokio::test]
async fn organization_authority_receipt_clock_is_sampled_after_the_port_answers() -> TestResult {
    let (entered, observed) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let bearer_token = bearer()?;
    let sign_in = HeldConfirmationPort {
        entered: Mutex::new(Some(entered)),
        release: Mutex::new(Some(released)),
        expected_bearer: bearer_token.clone(),
    };
    let clock = ControlledClock::new();
    let policy = policy()?;
    let request = verify_and_confirm_person(
        &sign_in,
        &clock,
        &policy,
        PersonTokenAudience::Admin,
        bearer_token,
    );
    tokio::pin!(request);
    tokio::select! {
        _answer = &mut request => return Err("person helper completed without awaiting confirmation".into()),
        entered = observed => {
            entered?;
            clock.set_millis(1_800_000_008_000);
            release
                .send(())
                .map_err(|_| "confirmation waiter disappeared")?;
        }
    }
    let proof = request.await?;
    let apply_at_millis = serde_json::from_value(json!(1_800_000_012_999_i64))?;
    assert!(
        proof
            .require_fresh(apply_at_millis, PersonConfirmationWindow::default())
            .is_ok()
    );
    Ok(())
}
