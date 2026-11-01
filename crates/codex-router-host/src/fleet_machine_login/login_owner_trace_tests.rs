//! Login outcome traces contain public identifiers and exclude credentials.
use super::*;

struct TraceCapture(Arc<std::sync::Mutex<Vec<u8>>>);
impl std::io::Write for TraceCapture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("trace lock poisoned"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[tokio::test]
async fn fleet_machine_login_records_public_outcomes_without_codes_or_credentials() {
    // Account for parallel untraced producers in the global interest registry.
    // With only one registered dispatch, tracing-core 0.1.36 consults the
    // registering thread's default and can cache Never from another test.
    // Keep both dispatches alive until every captured owner has joined.
    let untraced_dispatch = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let bytes = Arc::new(std::sync::Mutex::new(Vec::new()));
    let output = bytes.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || TraceCapture(output.clone()))
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    let fixture = Fixture::new();
    let running = fixture.start().await;
    let mut events = fixture.observe(&running);
    events.recv().await.unwrap();
    fixture.issue();
    assert!(matches!(
        fixture.next(&running, &mut events).await,
        Some(FleetLoginEvent::LoggedIn { .. })
    ));
    running.shutdown().await.unwrap();
    let fixture = Fixture::new();
    let running = fixture.start().await;
    let mut events = fixture.observe(&running);
    events.recv().await.unwrap();
    fixture.answers.send(Err(IssuerFailure::Timeout)).unwrap();
    fixture.clock.0.send_replace(5_000);
    assert!(matches!(
        events.recv().await,
        Some(FleetLoginEvent::OutcomeUnknown { .. })
    ));
    running.shutdown().await.unwrap();
    drop(guard);
    let text = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    for expected in [
        "machineLogin",
        "loggedIn",
        "outcomeUnknown",
        ORGANIZATION,
        ENROLLMENT,
    ] {
        assert!(
            text.contains(expected),
            "missing public trace fragment {expected}; captured: {text:?}"
        );
    }
    for secret in [
        "ABCD-EFGH",
        "device-secret-canary",
        "access-secret-canary",
        "refresh-secret-canary",
    ] {
        assert!(!text.contains(secret));
    }
    drop(untraced_dispatch);
}
