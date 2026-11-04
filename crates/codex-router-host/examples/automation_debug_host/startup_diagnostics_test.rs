use super::*;
use std::{
    io::Write,
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
struct CapturedDiagnostics(Arc<Mutex<Vec<u8>>>);

impl Write for CapturedDiagnostics {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("capture lock")
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'writer> tracing_subscriber::fmt::MakeWriter<'writer> for CapturedDiagnostics {
    type Writer = Self;
    fn make_writer(&'writer self) -> Self::Writer {
        self.clone()
    }
}

#[test]
fn startup_sink_excludes_unrelated_private_events() {
    // Arrange: use the same filtered layer as the opt-in normal Host.
    let capture = CapturedDiagnostics::default();
    let subscriber = tracing_subscriber::registry().with(diagnostic_layer(capture.clone()));

    // Act: retain approved static events and exclude an unrelated private event.
    tracing::subscriber::with_default(subscriber, || {
        tracing::warn!(private_payload = "PRIVATE_CANARY", "unrelated event");
        tracing::debug!(target: "codex_router_host::startup_diagnostics", {
            event.name = "codex_router.host.router_startup_child",
            child_state = "running", elapsed_ms = 250_u64 }, "Observed retained Router child");
        tracing::debug!(target: "acp_client_runtime::provider_diagnostics", {
            event.name = "codex_router.acp.transport_diagnostic",
            transport.stage = "outgoingFlushed", frame.kind = "prompt", observed_count = 1_usize,
        }, "Observed ACP transport boundary");
        tracing::debug!(target: "collaboration_runtime::native_settings_diagnostic", {
            event.name = "codex_router.native_settings_diagnostic",
            method = "thread/settings/update", request_id = 7_u64, outcome = "server_error",
        }, "Observed native compatibility outcome");
    });

    // Assert: the native diagnostic reaches the sink without widening unrelated capture.
    let bytes = capture.0.lock().expect("capture lock").clone();
    let output = String::from_utf8(bytes).expect("UTF-8 diagnostic");
    assert!(output.contains("router_startup_child"), "{output}");
    assert!(output.contains("outgoingFlushed"), "{output}");
    assert!(
        output.contains("codex_router.native_settings_diagnostic"),
        "{output}"
    );
    assert!(!output.contains("PRIVATE_CANARY"), "{output}");
    assert!(!output.contains("private_payload"), "{output}");
}
