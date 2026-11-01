//! Capture the actual policy diagnostics and its durable/wire projections without a real clock.
use super::*;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use tracing::instrument::WithSubscriber;

#[derive(Clone, Default)]
struct DiagnosticCapture(Arc<Mutex<Vec<u8>>>);
impl Write for DiagnosticCapture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("diagnostic capture unavailable"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn fleet_session_registrar_keeps_private_declared_settings_out_of_record_projections_and_logs()
 {
    let fixture = Fixture::new();
    let intent = fixture.intent();
    let mut declared = fixture.declared_input(&intent);
    let collaboration_contracts::HarnessDeclaredSettings::Native { effort, .. } =
        &mut declared.settings
    else {
        panic!("native fixture settings");
    };
    *effort = Some("PRIVATE_EFFORT_SENTINEL".to_owned().try_into().unwrap());
    declared.user_mcp =
        collaboration_contracts::HarnessMcpConfigurationIdentity::from_sha256([0xab; 32]);
    let identity = declared.identity().unwrap();
    let capture = DiagnosticCapture::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .finish();

    async {
        tracing::info!("diagnostic capture witness");
        fixture.registrar.prepare(&intent, &identity).await.unwrap();
        fixture
            .registrar
            .mark_dispatched(&intent.operation_id)
            .await
            .unwrap();
        fixture
            .registrar
            .record_provider_created(&intent.operation_id, fixture.session())
            .await
            .unwrap();
        fixture.grant();
        assert!(matches!(
            fixture
                .registrar
                .complete_created_setup(&intent.operation_id)
                .await
                .unwrap(),
            RegistrationCompletion::Registered(_)
        ));
    }
    .with_subscriber(subscriber)
    .await;

    let record = fixture
        .repository
        .records
        .lock()
        .unwrap()
        .first()
        .unwrap()
        .clone();
    let durable_projection = serde_json::json!({
        "launch": record.preparation().launch,
        "lifecycle": record.lifecycle(),
    })
    .to_string();
    let registration = fixture
        .authority
        .registrations
        .lock()
        .unwrap()
        .first()
        .unwrap()
        .clone();
    let diagnostics = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    assert!(diagnostics.contains("diagnostic capture witness"));
    for output in [
        durable_projection,
        String::from_utf8(registration).unwrap(),
        diagnostics,
    ] {
        for private_value in [
            "PRIVATE_EFFORT_SENTINEL",
            &"ab".repeat(32),
            "userMcpFingerprint",
            "preparedIdentities",
        ] {
            assert!(!output.contains(private_value));
        }
    }
}
