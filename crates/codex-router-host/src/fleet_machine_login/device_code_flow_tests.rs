use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, Ordering},
    },
};

use super::*;

struct ControlledClock(AtomicI64);
impl FleetLoginClock for ControlledClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
    fn wait_until(&self, deadline: SyncTime) -> LoginFuture<'_, Result<(), ()>> {
        Box::pin(async move {
            self.0.store(deadline.unix_millis(), Ordering::SeqCst);
            Ok(())
        })
    }
}

struct RecordingIssuer {
    clock: Arc<ControlledClock>,
    answers: Mutex<VecDeque<Result<DevicePollResult, IssuerFailure>>>,
    polls: Mutex<Vec<i64>>,
}
impl MachineTokenIssuer for RecordingIssuer {
    fn authorize<'a>(
        &'a self,
        _: &'a DeviceLoginRequest,
    ) -> LoginFuture<'a, Result<DeviceCodeAuthorization, IssuerFailure>> {
        Box::pin(async move {
            Ok(DeviceCodeAuthorization {
                device_code: SecretString::new("device-code-canary"),
                user_code: "ABCD-EFGH"
                    .to_owned()
                    .try_into()
                    .map_err(|_| IssuerFailure::InvalidResponse)?,
                verification_uri: "https://sign-in.example.invalid/activate"
                    .to_owned()
                    .try_into()
                    .map_err(|_| IssuerFailure::InvalidResponse)?,
                expires_at: SyncTime::from_unix_millis(600_000),
                interval_seconds: 5,
            })
        })
    }
    fn poll<'a>(
        &'a self,
        device_code: &'a SecretString,
    ) -> LoginFuture<'a, Result<DevicePollResult, IssuerFailure>> {
        Box::pin(async move {
            if device_code.expose_secret() != "device-code-canary" {
                return Err(IssuerFailure::InvalidResponse);
            }
            self.polls
                .lock()
                .map_err(|_| IssuerFailure::Unavailable)?
                .push(self.clock.now().unix_millis());
            self.answers
                .lock()
                .map_err(|_| IssuerFailure::Unavailable)?
                .pop_front()
                .unwrap_or(Err(IssuerFailure::InvalidResponse))
        })
    }
    fn refresh<'a>(
        &'a self,
        _: &'a RefreshCredentialBundle,
    ) -> LoginFuture<'a, Result<IssuedMachineToken, IssuerFailure>> {
        Box::pin(async { Err(IssuerFailure::InvalidGrant) })
    }
}

fn request() -> DeviceLoginRequest {
    DeviceLoginRequest {
        organization_id: "019f0000-0000-7000-8000-000000000001"
            .to_owned()
            .try_into()
            .unwrap(),
        installation: "019f0000-0000-7000-8000-000000000002"
            .to_owned()
            .try_into()
            .unwrap(),
        peer: serde_json::from_value(serde_json::json!("019f0000-0000-7000-8000-000000000003"))
            .unwrap(),
        label: "Fixture machine".to_owned().try_into().unwrap(),
    }
}

#[tokio::test]
async fn fleet_machine_login_polling_respects_pending_slow_down_and_timeout_on_controlled_time() {
    let clock = Arc::new(ControlledClock(AtomicI64::new(0)));
    let issuer = RecordingIssuer {
        clock: clock.clone(),
        answers: Mutex::new(VecDeque::from([
            Ok(DevicePollResult::Pending),
            Ok(DevicePollResult::SlowDown),
            Err(IssuerFailure::Timeout),
            Err(IssuerFailure::AccessDenied),
        ])),
        polls: Mutex::new(Vec::new()),
    };
    let mut observed_code = None;
    let result = run_device_code_login(
        &issuer,
        clock.as_ref(),
        &request(),
        RefreshCredentialKeeping::MemoryOnly,
        |code| observed_code = Some(code),
    )
    .await;
    assert!(matches!(
        result,
        Err(DeviceCodeFailure::OutcomeUnknown(
            FleetLoginUnknown::DeviceRedemption { .. }
        ))
    ));
    assert_eq!(*issuer.polls.lock().unwrap(), vec![5_000, 10_000, 20_000]);
    assert_eq!(
        issuer.answers.lock().unwrap().len(),
        1,
        "an uncertain redemption must not replay the code"
    );
    assert!(matches!(
        observed_code,
        Some(FleetLoginEvent::Code {
            keeping: RefreshCredentialKeeping::MemoryOnly,
            ..
        })
    ));
}

#[tokio::test]
async fn fleet_machine_login_expiry_stops_before_another_poll() {
    let clock = Arc::new(ControlledClock(AtomicI64::new(599_000)));
    let issuer = RecordingIssuer {
        clock: clock.clone(),
        answers: Mutex::new(VecDeque::new()),
        polls: Mutex::new(Vec::new()),
    };
    let result = run_device_code_login(
        &issuer,
        clock.as_ref(),
        &request(),
        RefreshCredentialKeeping::MemoryOnly,
        |_| {},
    )
    .await;
    assert!(matches!(
        result,
        Err(DeviceCodeFailure::Refused(
            FleetLoginRefusal::CodeExpired {}
        ))
    ));
    assert!(issuer.polls.lock().unwrap().is_empty());
    assert_eq!(clock.now(), SyncTime::from_unix_millis(600_000));
}

#[tokio::test]
async fn fleet_machine_login_known_issuer_denial_is_refused() {
    let clock = Arc::new(ControlledClock(AtomicI64::new(0)));
    let issuer = RecordingIssuer {
        clock: clock.clone(),
        answers: Mutex::new(VecDeque::from([Err(IssuerFailure::AccessDenied)])),
        polls: Mutex::new(Vec::new()),
    };
    let result = run_device_code_login(
        &issuer,
        clock.as_ref(),
        &request(),
        RefreshCredentialKeeping::MemoryOnly,
        |_| {},
    )
    .await;
    assert!(matches!(
        result,
        Err(DeviceCodeFailure::Refused(
            FleetLoginRefusal::AccessDenied {}
        ))
    ));
    assert_eq!(*issuer.polls.lock().unwrap(), vec![5_000]);
}

#[test]
fn fleet_machine_login_renews_with_one_sixth_remaining() {
    assert_eq!(
        renewal_deadline(
            SyncTime::from_unix_millis(1_000),
            SyncTime::from_unix_millis(601_000)
        ),
        SyncTime::from_unix_millis(501_000)
    );
}

proptest::proptest! {
    #[test]
    fn fleet_machine_login_renewal_deadline_stays_before_expiry(now in 0_i64..1_000_000_000, lifetime in 6_i64..3_600_001) {
        let deadline = renewal_deadline(SyncTime::from_unix_millis(now), SyncTime::from_unix_millis(now + lifetime)).unix_millis();
        proptest::prop_assert!(deadline > now);
        proptest::prop_assert!(deadline < now + lifetime);
        proptest::prop_assert!(deadline >= now + lifetime * 4 / 5);
    }
}
