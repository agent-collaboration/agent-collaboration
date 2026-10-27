use super::*;
#[cfg(feature = "quota-reset-test-harness")]
#[test]
fn storyboard_quota_diagnostics_omit_error_payloads() {
    let path = std::env::temp_dir().join(format!("quota-diagnostic-{}.log", std::process::id()));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("fresh diagnostic fixture file");
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter("off,quota_storyboard_diagnostic=warn")
        .with_writer(file)
        .without_time()
        .with_ansi(false)
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        record_storyboard_quota_error(
            "report_reload",
            &crate::quota_service_error::QuotaServiceError::StateStore(
                StateStoreError::CorruptQuotaSnapshot {
                    account_id: "private-account-sentinel".to_owned(),
                    field: "private-field-sentinel",
                },
            ),
        );
    });
    let evidence = std::fs::read_to_string(&path).expect("retained diagnostic");
    std::fs::remove_file(path).expect("diagnostic fixture cleanup");
    assert!(evidence.contains("report_reload"));
    assert!(evidence.contains("quota_error_variant"));
    assert!(evidence.contains("state_error_variant"));
    assert!(!evidence.contains("private-account-sentinel"));
    assert!(!evidence.contains("private-field-sentinel"));
}
