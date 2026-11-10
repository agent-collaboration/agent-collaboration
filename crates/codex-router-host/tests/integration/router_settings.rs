//! Owner-edited `settings.jsonc` as the Host's public loader reads it.

use codex_router_host::{load_or_create_provider_settings, router_settings_path};

#[test]
fn missing_settings_file_creates_enabled_provider_defaults() {
    let root = tempfile::tempdir().expect("router root");

    let configuration =
        load_or_create_provider_settings(root.path()).expect("default provider settings");

    assert!(configuration.claude().enabled);
    assert!(configuration.cursor().enabled);
    assert_eq!(configuration.claude().executable, None);
    assert_eq!(configuration.cursor().arguments, vec!["acp"]);
    let written = std::fs::read_to_string(router_settings_path(root.path()))
        .expect("default settings written");
    assert!(written.contains("\"providers\""), "{written}");
    assert!(written.contains("\"version\": 1"), "{written}");
    assert!(!root.path().join("providers.json").exists());
}

#[test]
fn unknown_provider_field_is_preserved_and_reports_its_path_and_section() {
    let root = tempfile::tempdir().expect("router root");
    let path = router_settings_path(root.path());
    let original = br#"{"providers": {"version":1,"providers":{"claude":{"enabled":true,"executable":null,"arguments":[]},"cursor":{"enabled":true,"executable":null,"arguments":["acp"]}},"surprise":true}}"#;
    std::fs::write(&path, original).expect("settings fixture");

    let error = load_or_create_provider_settings(root.path())
        .expect_err("unknown key must be rejected")
        .to_string();

    assert!(error.contains(&path.display().to_string()), "{error}");
    assert!(error.contains("\"providers\" section"), "{error}");
    assert_eq!(std::fs::read(&path).expect("preserved file"), original);
}
