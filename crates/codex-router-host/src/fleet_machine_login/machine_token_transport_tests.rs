use futures_util::FutureExt;

use super::super::login_clock::LoginFuture;
use super::*;
use collaboration_contracts::{MachineLoginGate, MachineLoginState};

struct ControlledClock(watch::Sender<i64>);
impl ControlledClock {
    fn new(now: i64) -> Self {
        Self(watch::channel(now).0)
    }
}
impl FleetLoginClock for ControlledClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(*self.0.borrow())
    }
    fn wait_until(&self, deadline: SyncTime) -> LoginFuture<'_, Result<(), ()>> {
        let mut time = self.0.subscribe();
        Box::pin(async move {
            while *time.borrow_and_update() < deadline.unix_millis() {
                time.changed().await.map_err(|_| ())?;
            }
            Ok(())
        })
    }
}

struct IdleConnection;
impl TransportConnection for IdleConnection {
    async fn send(&mut self, _: Bytes) -> Result<(), ConnectionClosed> {
        Ok(())
    }
    async fn receive(&mut self) -> Option<Result<Bytes, ConnectionClosed>> {
        std::future::pending().await
    }
    async fn close(self, _: CloseReason) {}
}

#[tokio::test]
async fn fleet_machine_login_idle_connection_closes_on_withdrawal_or_another_enrollment() {
    let clock = Arc::new(ControlledClock::new(0));
    let (sender, source) = MachineTokenSource::channel(clock);
    let original: EnrollmentId = "019f0000-0000-7000-8000-000000000001"
        .to_owned()
        .try_into()
        .unwrap();
    for replacement in [
        MachineAccessToken::NotLoggedIn,
        MachineAccessToken::Granted {
            enrollment_id: "019f0000-0000-7000-8000-000000000002"
                .to_owned()
                .try_into()
                .unwrap(),
            token: SecretString::new("new-enrollment-secret"),
            expires_at: SyncTime::from_unix_millis(1_000),
        },
    ] {
        sender.send_replace(MachineAccessToken::Granted {
            enrollment_id: original.clone(),
            token: SecretString::new("original-secret"),
            expires_at: SyncTime::from_unix_millis(1_000),
        });
        let mut connection = IdleConnection;
        let receiving = source.receive_for(&original, &mut connection);
        tokio::pin!(receiving);
        assert!(receiving.as_mut().now_or_never().is_none());

        sender.send_replace(replacement);

        assert_eq!(receiving.await, Some(Err(ConnectionClosed::Transport)));
    }
}

#[tokio::test]
async fn fleet_machine_login_token_source_never_keeps_a_grant_after_its_owner_disappears() {
    let clock = Arc::new(ControlledClock::new(0));
    let (sender, source) = MachineTokenSource::channel(clock);
    sender.send_replace(MachineAccessToken::Granted {
        enrollment_id: "019f0000-0000-7000-8000-000000000001"
            .to_owned()
            .try_into()
            .unwrap(),
        token: SecretString::new("orphaned-token-canary"),
        expires_at: SyncTime::from_unix_millis(1_000),
    });
    assert_eq!(source.machine_login_state(), MachineLoginState::LoggedIn);

    drop(sender);

    assert_eq!(source.machine_login_state(), MachineLoginState::NotLoggedIn);
    assert!(matches!(
        source.next_credential().await,
        Err(ConnectFailure::Unavailable)
    ));
}

#[tokio::test]
async fn fleet_machine_login_idle_receive_uses_the_rotated_expiry_and_closes_without_a_network_frame()
 {
    let clock = Arc::new(ControlledClock::new(0));
    let (sender, source) = MachineTokenSource::channel(clock.clone());
    let enrollment: EnrollmentId = "019f0000-0000-7000-8000-000000000001"
        .to_owned()
        .try_into()
        .unwrap();
    sender.send_replace(MachineAccessToken::Granted {
        enrollment_id: enrollment.clone(),
        token: SecretString::new("first-token"),
        expires_at: SyncTime::from_unix_millis(1_000),
    });
    let mut connection = IdleConnection;
    let receiving = source.receive_for(&enrollment, &mut connection);
    tokio::pin!(receiving);
    assert!(receiving.as_mut().now_or_never().is_none());
    sender.send_replace(MachineAccessToken::Granted {
        enrollment_id: enrollment.clone(),
        token: SecretString::new("rotated-token"),
        expires_at: SyncTime::from_unix_millis(2_000),
    });
    assert!(receiving.as_mut().now_or_never().is_none());
    clock.0.send_replace(1_000);
    assert!(
        receiving.as_mut().now_or_never().is_none(),
        "rotation extends the same connection's deadline"
    );

    clock.0.send_replace(2_000);

    assert_eq!(receiving.await, Some(Err(ConnectionClosed::Transport)));
    assert_eq!(source.machine_login_state(), MachineLoginState::NotLoggedIn);
}

#[tokio::test]
async fn fleet_machine_login_token_source_is_closed_before_login_and_uses_the_latest_grant() {
    let clock = Arc::new(ControlledClock::new(0));
    let (sender, source) = MachineTokenSource::channel(clock.clone());
    assert_eq!(source.machine_login_state(), MachineLoginState::NotLoggedIn);
    assert!(source.next_credential().now_or_never().is_none());

    sender.send_replace(MachineAccessToken::Granted {
        enrollment_id: "019f0000-0000-7000-8000-000000000001"
            .to_owned()
            .try_into()
            .unwrap(),
        token: SecretString::new("first-token-canary"),
        expires_at: SyncTime::from_unix_millis(1_000),
    });
    assert_eq!(source.machine_login_state(), MachineLoginState::LoggedIn);
    assert_eq!(
        source.next_credential().await.unwrap().credential.expose(),
        "Bearer first-token-canary"
    );
    sender.send_replace(MachineAccessToken::Granted {
        enrollment_id: "019f0000-0000-7000-8000-000000000001"
            .to_owned()
            .try_into()
            .unwrap(),
        token: SecretString::new("second-token-canary"),
        expires_at: SyncTime::from_unix_millis(2_000),
    });
    assert_eq!(
        source.next_credential().await.unwrap().credential.expose(),
        "Bearer second-token-canary"
    );

    clock.0.send_replace(2_000);
    assert_eq!(source.machine_login_state(), MachineLoginState::NotLoggedIn);
    assert!(source.next_credential().now_or_never().is_none());
    drop(sender);
    assert!(matches!(
        source.next_credential().await,
        Err(ConnectFailure::Unavailable)
    ));
}
