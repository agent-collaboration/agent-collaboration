//! Conformance-only observation of real issuer I/O, signatures and person policy.
use crate::{
    CentralPlatform, IssuerKeyCache, IssuerKeySource, IssuerPersonSignInClient,
    PersonSessionConfirmer, ServiceTokenTimeRefusal, ServiceTokenTimeRule,
    grant_deadline_is_current,
    organization_authority::{
        PersonConfirmationRefusal, PersonSessionConfirmation, PersonTokenAudience,
        PersonTokenPolicy, PersonTokenRefusal, SignInUnavailable,
        decode_person_session_confirmation, verify_and_confirm_person,
    },
    validate_service_token_times,
};
use fleet_identity_protocol::UnixSeconds;
use runtime_primitives::{Clock, SyncTime};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicI64, AtomicUsize, Ordering},
};

pub const PERSON_PROOF_PROBE_PATH: &str = "/__conformance/person-proof";

// Trust policy and the key source are supplied separately by the host's fixture
// configuration. Neither can be selected by these untrusted token bytes.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonProofProbe {
    pub steps: Vec<PersonProofStep>,
    pub windows: Vec<ServiceWindowProbe>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonProofStep {
    bearer_token: String,
    now_millis: i64,
    confirmation: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServiceWindowProbe {
    rule: WindowRule,
    issued_at: i64,
    expires_at: i64,
    now: i64,
    grant_expires_at: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum WindowRule {
    Service,
    IssuerDecision,
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonProofObservation {
    pub steps: Vec<PersonStepObservation>,
    pub windows: Vec<WindowObservation>,
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonStepObservation {
    pub result: PersonProofResult,
    pub confirmation_calls: usize,
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PersonProofResult {
    Valid,
    Malformed,
    UnsupportedAlgorithm,
    BadSignature,
    WrongTokenType,
    WrongIssuer,
    WrongAudience,
    WrongClient,
    Expired,
    NotYetValid,
    NotAPerson,
    NoOrganization,
    InvalidConfiguration,
    Unavailable,
    Inactive,
    MismatchedConfirmation,
    InvalidConfirmation,
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowObservation {
    Valid,
    InvalidInterval,
    LifetimeExceeded,
    NotYetValid,
    Expired,
    GrantExpired,
}

struct ControlledClock(AtomicI64);
impl Clock for ControlledClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}

// This stands in only for authenticated introspection. Its producer-owned K5
// bytes go through the real decoder, and it refuses a different bearer value.
struct ConfirmationFixture {
    expected_bearer: String,
    body: Vec<u8>,
    calls: Arc<AtomicUsize>,
}

impl PersonSessionConfirmer for ConfirmationFixture {
    async fn confirm_person_session<'request>(
        &'request self,
        bearer: &'request str,
    ) -> Result<PersonSessionConfirmation, SignInUnavailable> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if bearer != self.expected_bearer {
            return Err(SignInUnavailable::ConfirmationUnreachable);
        }
        decode_person_session_confirmation(&self.body)
            .map_err(|_| SignInUnavailable::ConfirmationUnreachable)
    }
}

pub async fn observe_person_proof<
    Platform: CentralPlatform + Clone,
    Source: IssuerKeySource + Clone,
>(
    platform: Platform,
    source: Source,
    policy: &PersonTokenPolicy,
    input: PersonProofProbe,
) -> PersonProofObservation {
    let clock = Arc::new(ControlledClock(AtomicI64::new(0)));
    let keys = Arc::new(IssuerKeyCache::default());
    let mut steps = Vec::with_capacity(input.steps.len());
    for step in input.steps {
        clock.0.store(step.now_millis, Ordering::SeqCst);
        let calls = Arc::new(AtomicUsize::new(0));
        let client = IssuerPersonSignInClient::new(
            platform.clone(),
            source.clone(),
            keys.clone(),
            ConfirmationFixture {
                expected_bearer: step.bearer_token.clone(),
                body: step.confirmation.into_bytes(),
                calls: calls.clone(),
            },
            clock.clone(),
        );
        let result = match verify_and_confirm_person(
            &client,
            clock.as_ref(),
            policy,
            PersonTokenAudience::Admin,
            step.bearer_token,
        )
        .await
        {
            Ok(_) => PersonProofResult::Valid,
            Err(refusal) => person_result(refusal),
        };
        steps.push(PersonStepObservation {
            result,
            confirmation_calls: calls.load(Ordering::SeqCst),
        });
    }
    let windows = input.windows.into_iter().map(observe_window).collect();
    PersonProofObservation { steps, windows }
}

fn person_result(refusal: PersonConfirmationRefusal) -> PersonProofResult {
    use PersonProofResult as Result;
    match refusal {
        PersonConfirmationRefusal::InvalidToken(refusal) => match refusal {
            PersonTokenRefusal::Malformed => Result::Malformed,
            PersonTokenRefusal::WrongTokenType => Result::WrongTokenType,
            PersonTokenRefusal::UnsupportedAlgorithm => Result::UnsupportedAlgorithm,
            PersonTokenRefusal::BadSignature => Result::BadSignature,
            PersonTokenRefusal::WrongIssuer => Result::WrongIssuer,
            PersonTokenRefusal::WrongAudience => Result::WrongAudience,
            PersonTokenRefusal::WrongClient => Result::WrongClient,
            PersonTokenRefusal::Expired => Result::Expired,
            PersonTokenRefusal::NotYetValid => Result::NotYetValid,
            PersonTokenRefusal::NotAPerson => Result::NotAPerson,
            PersonTokenRefusal::NoOrganization => Result::NoOrganization,
            PersonTokenRefusal::InvalidConfiguration => Result::InvalidConfiguration,
        },
        PersonConfirmationRefusal::Unavailable(_) => Result::Unavailable,
        PersonConfirmationRefusal::Inactive => Result::Inactive,
        PersonConfirmationRefusal::MismatchedProof => Result::MismatchedConfirmation,
        PersonConfirmationRefusal::MalformedReply
        | PersonConfirmationRefusal::Stale
        | PersonConfirmationRefusal::InvalidWindow => Result::InvalidConfirmation,
    }
}

fn observe_window(input: ServiceWindowProbe) -> WindowObservation {
    let rule = match input.rule {
        WindowRule::Service => ServiceTokenTimeRule::Service,
        WindowRule::IssuerDecision => ServiceTokenTimeRule::IssuerDecision,
    };
    match validate_service_token_times(
        rule,
        UnixSeconds(input.issued_at),
        UnixSeconds(input.expires_at),
        UnixSeconds(input.now),
    ) {
        Err(ServiceTokenTimeRefusal::InvalidInterval) => WindowObservation::InvalidInterval,
        Err(ServiceTokenTimeRefusal::LifetimeExceeded) => WindowObservation::LifetimeExceeded,
        Err(ServiceTokenTimeRefusal::NotYetValid) => WindowObservation::NotYetValid,
        Err(ServiceTokenTimeRefusal::Expired) => WindowObservation::Expired,
        Ok(()) => {
            if input.grant_expires_at.is_some_and(|deadline| {
                !grant_deadline_is_current(UnixSeconds(input.now), UnixSeconds(deadline))
            }) {
                WindowObservation::GrantExpired
            } else {
                WindowObservation::Valid
            }
        }
    }
}
