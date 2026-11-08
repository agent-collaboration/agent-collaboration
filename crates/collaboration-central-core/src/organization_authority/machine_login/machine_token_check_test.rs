//! C9 with real Ed25519 signatures, B2's key cache, resolution and verifier,
//! a counting key source and a controlled clock. Each case changes one claim
//! or header from a valid token and holds every other bound valid.
use super::super::super::fleet_statement_test_support::RealEd25519;
use super::super::in_memory_enrollments::{EnrollmentStore, InMemoryEnrollments};
use super::super::test_fixtures::{
    MACHINE, NOW_MS, NOW_S, TestClock, client, empty_store, enrollment, installation, settings,
    uuid7, with_enrolled,
};
use super::{
    ISSUER_CLOCK_SKEW_SECONDS, MachineTokenAdmission, MachineTokenCheckRefusal, MachineTokenPolicy,
    MachineTokenVerifier, admit_machine_token, renew_machine_token,
};
use crate::identity_contract::{
    AdmittedMachine, MachineConnectionAdmission, MachineConnectionRefusal, PresentedMachine,
};
use crate::{IssuerKeyCache, IssuerKeySource, IssuerSignatureVerifier, KeyFetchFailure, RawJwks};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use fleet_identity_protocol::{MachineTokenRenewal, MachineTokenRenewalRefusal, UnixSeconds};
use fleet_sync_protocol::PeerId;
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const WORKING: u64 = 0x40;
const ISSUER: &str = "https://sign-in.example.invalid";
const AUDIENCE: &str = "https://central.example.invalid/machine";
const KID: &str = "machine-fixture";

/// Public test fixtures only: never a deployment signing key.
fn signing_key(seed: u8) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap()
}

/// The issuer's published keys: `kid → seed`. Counts fetches; can fail; can
/// run a hook during a fetch (a slow key source).
struct TestKeys {
    published: Mutex<Vec<(&'static str, u8)>>,
    fetches: AtomicUsize,
    unreachable: bool,
    during_fetch: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl TestKeys {
    fn publishing(published: Vec<(&'static str, u8)>) -> Arc<Self> {
        Arc::new(Self {
            published: Mutex::new(published),
            fetches: AtomicUsize::new(0),
            unreachable: false,
            during_fetch: Mutex::new(None),
        })
    }
}
impl IssuerKeySource for Arc<TestKeys> {
    fn configured_source(&self) -> &str {
        "https://sign-in.example.invalid/api/auth/jwks"
    }
    async fn fetch_issuer_keys(&self) -> Result<RawJwks, KeyFetchFailure> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        if let Some(hook) = self.during_fetch.lock().unwrap().take() {
            hook();
        }
        if self.unreachable {
            return Err(KeyFetchFailure::Unreachable);
        }
        let keys = self
            .published
            .lock()
            .unwrap()
            .iter()
            .map(|(kid, seed)| {
                json!({"kid":kid,"kty":"OKP","crv":"Ed25519","alg":"EdDSA",
                    "x":URL_SAFE_NO_PAD.encode(signing_key(*seed).public_key().as_ref())})
            })
            .collect::<Vec<_>>();
        Ok(RawJwks {
            body: serde_json::to_vec(&json!({ "keys": keys })).unwrap(),
            cache_control: None,
        })
    }
}

fn verifier(
    keys: &Arc<TestKeys>,
    clock: &Arc<TestClock>,
) -> MachineTokenVerifier<RealEd25519, Arc<TestKeys>> {
    let clock: Arc<dyn runtime_primitives::Clock> = clock.clone();
    MachineTokenVerifier::new(
        IssuerSignatureVerifier::new(
            RealEd25519,
            keys.clone(),
            Arc::new(IssuerKeyCache::default()),
            clock.clone(),
        ),
        clock,
        MachineTokenPolicy {
            issuer: ISSUER.to_owned(),
            machine_audience: AUDIENCE.to_owned(),
        },
    )
}

fn header() -> Value {
    json!({"alg":"EdDSA","kid":KID,"typ":"JWT"})
}

/// A valid token for the working enrollment of `MACHINE`, with the provider's
/// other registered claims beside the machine ones.
fn claims() -> Value {
    json!({
        "iss": ISSUER, "sub": uuid7(0x02), "aud": AUDIENCE,
        "iat": NOW_S, "exp": NOW_S + 3600, "jti": uuid7(0x60),
        "client_id": uuid7(WORKING), "azp": uuid7(WORKING), "scope": "offline_access",
        "principal": {"kind":"machine","installation_id":uuid7(MACHINE),"enrollment_id":uuid7(WORKING)},
        "organization_id": uuid7(0x01),
        "grant_expires_at": NOW_S + 1800,
    })
}

fn sign(header: &Value, claims: &Value, seed: u8) -> String {
    let signed = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(header).unwrap()),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap())
    );
    format!(
        "{signed}.{}",
        URL_SAFE_NO_PAD.encode(signing_key(seed).sign(signed.as_bytes()).as_ref())
    )
}

fn token_with(change: impl FnOnce(&mut Value)) -> String {
    let mut body = claims();
    change(&mut body);
    sign(&header(), &body, 7)
}

fn presented() -> PresentedMachine {
    PresentedMachine {
        installation: installation(MACHINE),
        peer: PeerId::try_from(uuid7(MACHINE)).unwrap(),
    }
}

fn working() -> EnrollmentStore {
    with_enrolled(empty_store(), MACHINE, WORKING, 1, NOW_S - 60)
}

/// Checks `token` at `now_ms` against a fresh cache publishing seed 7 as `KID`.
async fn check_at(token: &str, now_ms: i64) -> Result<(), MachineTokenCheckRefusal> {
    let keys = TestKeys::publishing(vec![(KID, 7)]);
    let clock = TestClock::at(now_ms);
    verifier(&keys, &clock).verify(token).await.map(|_| ())
}

async fn check(token: &str) -> Result<(), MachineTokenCheckRefusal> {
    check_at(token, NOW_MS).await
}

#[tokio::test]
async fn a_valid_machine_token_with_unread_registered_claims_is_admitted() {
    let keys = TestKeys::publishing(vec![(KID, 7)]);
    let clock = TestClock::at(NOW_MS);
    let port = InMemoryEnrollments::with(working());

    let admission = admit_machine_token(
        &verifier(&keys, &clock),
        &port,
        &token_with(|_| {}),
        presented(),
        settings(),
    )
    .await
    .unwrap();

    assert_eq!(
        admission,
        MachineTokenAdmission::Admission(MachineConnectionAdmission::Admitted {
            machine: AdmittedMachine {
                installation: installation(MACHINE),
                peer: PeerId::try_from(uuid7(MACHINE)).unwrap(),
                organization_id: uuid7(0x01).try_into().unwrap(),
                enrollment_id: enrollment(WORKING),
                // The earlier of exp + 60 (NOW_S + 3660) and the grant (NOW_S + 1800).
                token_expires_at: UnixSeconds(NOW_S + 1800),
            },
        })
    );
}

#[tokio::test]
async fn only_an_eddsa_header_naming_a_key_is_allowed() {
    let body = claims();
    for header in [
        json!({"alg":"ES256","kid":KID}),
        json!({"alg":"none","kid":KID}),
        json!({"alg":"EdDSA","kid":""}),
        json!({"alg":"EdDSA","kid":KID,"typ":"dpop+jwt"}),
    ] {
        assert_eq!(
            check(&sign(&header, &body, 7)).await,
            Err(MachineTokenCheckRefusal::UnsupportedHeader),
            "{header}"
        );
    }
    assert_eq!(
        check("not.a-token").await,
        Err(MachineTokenCheckRefusal::Malformed)
    );
}

#[tokio::test]
async fn a_token_from_another_issuer_is_refused_before_its_signature() {
    let keys = TestKeys::publishing(vec![(KID, 7)]);
    let clock = TestClock::at(NOW_MS);
    let token = token_with(|claims| claims["iss"] = json!("https://elsewhere.example.invalid"));

    assert_eq!(
        verifier(&keys, &clock).verify(&token).await,
        Err(MachineTokenCheckRefusal::Issuer)
    );
    assert_eq!(keys.fetches.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_bad_signature_and_an_unreachable_key_source_are_different_refusals() {
    // Signed by a key the issuer never published under this kid.
    let forged = sign(&header(), &claims(), 9);
    assert_eq!(
        check(&forged).await,
        Err(MachineTokenCheckRefusal::Signature)
    );

    let unreachable = Arc::new(TestKeys {
        published: Mutex::new(vec![(KID, 7)]),
        fetches: AtomicUsize::new(0),
        unreachable: true,
        during_fetch: Mutex::new(None),
    });
    let clock = TestClock::at(NOW_MS);
    assert_eq!(
        verifier(&unreachable, &clock)
            .verify(&token_with(|_| {}))
            .await,
        Err(MachineTokenCheckRefusal::VerificationUnavailable)
    );
}

/// A warm cache answers a known key without fetching; an unknown kid causes
/// exactly one refetch, which finds a key rotated in since.
#[tokio::test]
async fn a_rotated_key_is_found_with_one_refetch_of_a_warm_cache() {
    let keys = TestKeys::publishing(vec![(KID, 7)]);
    let clock = TestClock::at(NOW_MS);
    let checker = verifier(&keys, &clock);
    checker.verify(&token_with(|_| {})).await.unwrap();
    checker.verify(&token_with(|_| {})).await.unwrap();
    assert_eq!(keys.fetches.load(Ordering::SeqCst), 1);

    keys.published.lock().unwrap().push(("rotated", 8));
    let rotated = sign(&json!({"alg":"EdDSA","kid":"rotated"}), &claims(), 8);
    checker.verify(&rotated).await.unwrap();

    assert_eq!(keys.fetches.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn the_token_must_name_centrals_machine_address() {
    let other = token_with(|claims| claims["aud"] = json!("https://central.example.invalid/admin"));
    assert_eq!(check(&other).await, Err(MachineTokenCheckRefusal::Audience));
    let named = token_with(|claims| {
        claims["aud"] = json!(["https://central.example.invalid/admin", AUDIENCE]);
    });
    assert_eq!(check(&named).await, Ok(()));
}

#[tokio::test]
async fn exp_allows_sixty_seconds_of_skew_and_no_more() {
    let exp = NOW_S + 3600;
    let token = token_with(|claims| claims["grant_expires_at"] = json!(exp + 600));
    let deadline_ms = (exp + ISSUER_CLOCK_SKEW_SECONDS) * 1000;
    assert_eq!(check_at(&token, deadline_ms - 1).await, Ok(()));
    assert_eq!(
        check_at(&token, deadline_ms).await,
        Err(MachineTokenCheckRefusal::Expired)
    );
}

#[tokio::test]
async fn iat_may_be_at_most_sixty_seconds_ahead() {
    let at_skew = token_with(|claims| claims["iat"] = json!(NOW_S + ISSUER_CLOCK_SKEW_SECONDS));
    assert_eq!(check(&at_skew).await, Ok(()));
    let beyond = token_with(|claims| claims["iat"] = json!(NOW_S + ISSUER_CLOCK_SKEW_SECONDS + 1));
    assert_eq!(
        check(&beyond).await,
        Err(MachineTokenCheckRefusal::IssuedInFuture)
    );
}

#[tokio::test]
async fn the_grant_expiry_has_no_leeway() {
    let token = token_with(|_| {});
    let grant_ms = (NOW_S + 1800) * 1000;
    assert_eq!(check_at(&token, grant_ms - 1).await, Ok(()));
    assert_eq!(
        check_at(&token, grant_ms).await,
        Err(MachineTokenCheckRefusal::GrantExpired)
    );
}

/// The clock is read after the key fetch: a slow fetch that crosses the
/// token's expiry refuses it.
#[tokio::test]
async fn time_is_checked_on_the_clock_read_after_the_key_fetch() {
    let keys = TestKeys::publishing(vec![(KID, 7)]);
    let clock = TestClock::at(NOW_MS);
    let during = clock.clone();
    *keys.during_fetch.lock().unwrap() = Some(Box::new(move || during.set((NOW_S + 1800) * 1000)));

    assert_eq!(
        verifier(&keys, &clock).verify(&token_with(|_| {})).await,
        Err(MachineTokenCheckRefusal::GrantExpired)
    );
}

#[tokio::test]
async fn the_machine_claims_stay_typed() {
    for change in [
        |claims: &mut Value| claims["principal"]["kind"] = json!("person"),
        |claims: &mut Value| claims["principal"]["extra"] = json!(true),
        |claims: &mut Value| claims["organization_id"] = json!("not-an-id"),
        |claims: &mut Value| {
            if let Some(fields) = claims.as_object_mut() {
                fields.remove("client_id");
            }
        },
    ] {
        assert_eq!(
            check(&token_with(change)).await,
            Err(MachineTokenCheckRefusal::Claims)
        );
    }
}

#[tokio::test]
async fn a_valid_token_for_a_client_the_enrollment_never_recorded_is_refused_at_admission() {
    let keys = TestKeys::publishing(vec![(KID, 7)]);
    let clock = TestClock::at(NOW_MS);
    let port = InMemoryEnrollments::with(working());
    let token = token_with(|claims| claims["client_id"] = json!(uuid7(0x99)));

    let admission = admit_machine_token(
        &verifier(&keys, &clock),
        &port,
        &token,
        presented(),
        settings(),
    )
    .await
    .unwrap();

    assert_eq!(
        admission,
        MachineTokenAdmission::Admission(MachineConnectionAdmission::Refused {
            reason: MachineConnectionRefusal::WrongClient {},
        })
    );
}

fn open_connection() -> AdmittedMachine {
    AdmittedMachine {
        installation: installation(MACHINE),
        peer: PeerId::try_from(uuid7(MACHINE)).unwrap(),
        organization_id: uuid7(0x01).try_into().unwrap(),
        enrollment_id: enrollment(WORKING),
        token_expires_at: UnixSeconds(NOW_S + 60),
    }
}

async fn renew(store: EnrollmentStore, token: &str) -> MachineTokenRenewal {
    let keys = TestKeys::publishing(vec![(KID, 7)]);
    let clock = TestClock::at(NOW_MS);
    let port = InMemoryEnrollments::with(store);
    renew_machine_token(
        &verifier(&keys, &clock),
        &port,
        token,
        &open_connection(),
        settings(),
    )
    .await
    .unwrap()
}

fn renewal_refused(reason: MachineTokenRenewalRefusal) -> MachineTokenRenewal {
    MachineTokenRenewal::Refused { reason }
}

#[tokio::test]
async fn a_fresh_token_for_the_connections_own_enrollment_renews_it() {
    assert_eq!(
        renew(working(), &token_with(|_| {})).await,
        MachineTokenRenewal::Renewed {}
    );
}

#[tokio::test]
async fn a_fresh_token_for_another_machine_or_enrollment_does_not_renew_this_connection() {
    let other_machine = token_with(|claims| {
        // Another machine's own valid token: its installation, enrollment and client.
        claims["principal"]["installation_id"] = json!(uuid7(0x11));
        claims["principal"]["enrollment_id"] = json!(uuid7(0x45));
        claims["client_id"] = json!(uuid7(0x45));
    });
    assert_eq!(
        renew(working(), &other_machine).await,
        renewal_refused(MachineTokenRenewalRefusal::WrongInstallation {})
    );
    let other_enrollment = token_with(|claims| {
        claims["principal"]["enrollment_id"] = json!(uuid7(0x41));
        claims["client_id"] = json!(uuid7(0x41));
    });
    assert_eq!(
        renew(working(), &other_enrollment).await,
        renewal_refused(MachineTokenRenewalRefusal::EnrollmentNotActive {})
    );
}

#[tokio::test]
async fn renewal_answers_each_token_and_enrollment_refusal_in_its_own_kind() {
    let expired = token_with(|claims| claims["grant_expires_at"] = json!(NOW_S));
    assert_eq!(
        renew(working(), &expired).await,
        renewal_refused(MachineTokenRenewalRefusal::TokenExpired {})
    );
    assert_eq!(
        renew(working(), &sign(&header(), &claims(), 9)).await,
        renewal_refused(MachineTokenRenewalRefusal::TokenInvalid {})
    );
    let wrong_client = token_with(|claims| claims["client_id"] = json!(uuid7(0x99)));
    assert_eq!(
        renew(working(), &wrong_client).await,
        renewal_refused(MachineTokenRenewalRefusal::WrongClient {})
    );
    let mut revoked = working();
    if let Some(row) = revoked.enrollments.get_mut(&enrollment(WORKING)) {
        row.state = super::super::enrollment_records::StoredEnrollmentState::Revoked {
            machine_client_id: client(WORKING),
            reason: crate::identity_contract::EnrollmentRevocationReason::Owner {},
        };
    }
    if let Some(row) = revoked.installations.get_mut(&installation(MACHINE)) {
        row.active = None;
    }
    assert_eq!(
        renew(revoked, &token_with(|_| {})).await,
        renewal_refused(MachineTokenRenewalRefusal::EnrollmentNotActive {})
    );
}

/// A token checked valid before the transaction, then held until `release_ms`
/// before its admission reads the enrollment (advisor point 01a1bc73-34bc).
/// Also says whether the store is exactly as it was.
async fn admit_after_wait(token: &str, release_ms: i64) -> (MachineTokenAdmission, bool) {
    let keys = TestKeys::publishing(vec![(KID, 7)]);
    let clock = TestClock::at(NOW_MS);
    let port = InMemoryEnrollments::with(working());
    let held = clock.clone();
    port.before_next_decision(move || held.set(release_ms));
    let admission = admit_machine_token(
        &verifier(&keys, &clock),
        &port,
        token,
        presented(),
        settings(),
    )
    .await
    .unwrap();
    (admission, port.snapshot() == working())
}

/// The open connection being renewed, its own lifetime well past every
/// release point, so only the fresh token's bounds can decide.
fn long_lived_connection() -> AdmittedMachine {
    AdmittedMachine {
        token_expires_at: UnixSeconds(NOW_S + 10_000),
        ..open_connection()
    }
}

async fn renew_after_wait(token: &str, release_ms: i64) -> (MachineTokenRenewal, bool) {
    let keys = TestKeys::publishing(vec![(KID, 7)]);
    let clock = TestClock::at(NOW_MS);
    let port = InMemoryEnrollments::with(working());
    let held = clock.clone();
    port.before_next_decision(move || held.set(release_ms));
    let renewal = renew_machine_token(
        &verifier(&keys, &clock),
        &port,
        token,
        &long_lived_connection(),
        settings(),
    )
    .await
    .unwrap();
    (renewal, port.snapshot() == working())
}

fn admitted_until(admission: &MachineTokenAdmission) -> Option<UnixSeconds> {
    match admission {
        MachineTokenAdmission::Admission(MachineConnectionAdmission::Admitted { machine }) => {
            Some(machine.token_expires_at)
        }
        _ => None,
    }
}

fn bounded(exp: i64, grant: i64) -> String {
    token_with(|claims| {
        claims["exp"] = json!(exp);
        claims["grant_expires_at"] = json!(grant);
    })
}

/// exp + 60 ends first: exp = NOW_S + 100, the grant much later. Just before
/// the bound the clock is past the raw exp, inside the skew window.
const EXP_BOUND_S: i64 = NOW_S + 100 + ISSUER_CLOCK_SKEW_SECONDS;
fn exp_bound_token() -> String {
    bounded(NOW_S + 100, NOW_S + 1000)
}
/// The grant ends first, with no leeway.
const GRANT_BOUND_S: i64 = NOW_S + 100;
fn grant_bound_token() -> String {
    bounded(NOW_S + 3600, NOW_S + 100)
}

/// 1 ms before, at, and 1 ms after the bound. Every instant is observed before
/// the assertion, so a red shows all three. The flag is full typed equality
/// of the store with its state before (`EnrollmentStore: Eq`).
async fn admissions_around(token: &str, bound_s: i64) -> Vec<(MachineTokenAdmission, bool)> {
    let mut observed = Vec::new();
    for release_ms in [bound_s * 1000 - 1, bound_s * 1000, bound_s * 1000 + 1] {
        observed.push(admit_after_wait(token, release_ms).await);
    }
    observed
}

async fn renewals_around(token: &str, bound_s: i64) -> Vec<(MachineTokenRenewal, bool)> {
    let mut observed = Vec::new();
    for release_ms in [bound_s * 1000 - 1, bound_s * 1000, bound_s * 1000 + 1] {
        observed.push(renew_after_wait(token, release_ms).await);
    }
    observed
}

fn admitted(until_s: i64) -> MachineTokenAdmission {
    MachineTokenAdmission::Admission(MachineConnectionAdmission::Admitted {
        machine: AdmittedMachine {
            installation: installation(MACHINE),
            peer: PeerId::try_from(uuid7(MACHINE)).unwrap(),
            organization_id: uuid7(0x01).try_into().unwrap(),
            enrollment_id: enrollment(WORKING),
            token_expires_at: UnixSeconds(until_s),
        },
    })
}

#[tokio::test]
async fn admission_rechecks_exp_plus_skew_on_the_clock_read_after_waiting() {
    let refused = MachineTokenAdmission::TokenRefused(MachineTokenCheckRefusal::Expired);
    assert_eq!(
        admissions_around(&exp_bound_token(), EXP_BOUND_S).await,
        vec![
            (admitted(EXP_BOUND_S), true),
            (refused.clone(), true),
            (refused, true)
        ]
    );
}

#[tokio::test]
async fn admission_rechecks_the_grant_on_the_clock_read_after_waiting() {
    let refused = MachineTokenAdmission::TokenRefused(MachineTokenCheckRefusal::GrantExpired);
    assert_eq!(
        admissions_around(&grant_bound_token(), GRANT_BOUND_S).await,
        vec![
            (admitted(GRANT_BOUND_S), true),
            (refused.clone(), true),
            (refused, true)
        ]
    );
}

#[tokio::test]
async fn renewal_rechecks_exp_plus_skew_on_the_clock_read_after_waiting() {
    let expired = renewal_refused(MachineTokenRenewalRefusal::TokenExpired {});
    assert_eq!(
        renewals_around(&exp_bound_token(), EXP_BOUND_S).await,
        vec![
            (MachineTokenRenewal::Renewed {}, true),
            (expired.clone(), true),
            (expired, true)
        ]
    );
}

#[tokio::test]
async fn renewal_rechecks_the_grant_on_the_clock_read_after_waiting() {
    let expired = renewal_refused(MachineTokenRenewalRefusal::TokenExpired {});
    assert_eq!(
        renewals_around(&grant_bound_token(), GRANT_BOUND_S).await,
        vec![
            (MachineTokenRenewal::Renewed {}, true),
            (expired.clone(), true),
            (expired, true)
        ]
    );
}

/// The host's timer is the earlier of exp + 60 and the grant, never the raw
/// exp: each order, and a tie.
#[tokio::test]
async fn the_admitted_timer_is_the_earlier_of_exp_plus_skew_and_the_grant() {
    for (exp, grant, until) in [
        (NOW_S + 100, NOW_S + 1000, NOW_S + 160),
        (NOW_S + 3600, NOW_S + 100, NOW_S + 100),
        (NOW_S + 100, NOW_S + 160, NOW_S + 160),
    ] {
        let (admission, _) = admit_after_wait(&bounded(exp, grant), NOW_MS).await;
        assert_eq!(
            admitted_until(&admission),
            Some(UnixSeconds(until)),
            "exp {exp}, grant {grant}"
        );
    }
}
