use std::sync::atomic::{AtomicI64, Ordering};

use futures_util::FutureExt;

use super::super::login_clock::LoginFuture;
use super::*;
use collaboration_contracts::{MachineLoginGate, MachineLoginState};

struct ControlledClock(AtomicI64);
impl FleetLoginClock for ControlledClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
    fn wait_until(&self, _: SyncTime) -> LoginFuture<'_, Result<(), ()>> {
        Box::pin(std::future::pending())
    }
}

#[tokio::test]
async fn fleet_machine_login_token_source_is_closed_before_login_and_uses_the_latest_grant() {
    let clock = Arc::new(ControlledClock(AtomicI64::new(0)));
    let (sender, source) = MachineTokenSource::channel(clock.clone());
    assert_eq!(source.machine_login_state(), MachineLoginState::NotLoggedIn);
    assert!(source.next_credential().now_or_never().is_none());

    sender.send_replace(MachineAccessToken::Granted {
        token: SecretString::new("first-token-canary"),
        expires_at: SyncTime::from_unix_millis(1_000),
    });
    assert_eq!(source.machine_login_state(), MachineLoginState::LoggedIn);
    assert_eq!(
        source.next_credential().await.unwrap().expose(),
        "Bearer first-token-canary"
    );
    sender.send_replace(MachineAccessToken::Granted {
        token: SecretString::new("second-token-canary"),
        expires_at: SyncTime::from_unix_millis(2_000),
    });
    assert_eq!(
        source.next_credential().await.unwrap().expose(),
        "Bearer second-token-canary"
    );

    clock.0.store(2_000, Ordering::SeqCst);
    assert_eq!(source.machine_login_state(), MachineLoginState::NotLoggedIn);
    assert!(source.next_credential().now_or_never().is_none());
    drop(sender);
    assert!(matches!(
        source.next_credential().await,
        Err(ConnectFailure::Unavailable)
    ));
}
