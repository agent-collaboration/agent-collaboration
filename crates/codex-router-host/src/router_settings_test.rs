use super::*;
use crate::port_configuration::PortSettingsSection;

fn write_settings(root: &Path, text: &str) -> PathBuf {
    let path = router_settings_path(root);
    fs::write(&path, text).expect("settings fixture");
    path
}

const VALID_PROVIDERS: &str = r#"{"version":1,"providers":{"claude":{"enabled":false,"executable":"/configured/claude","arguments":[]},"cursor":{"enabled":true,"executable":null,"arguments":["acp"]}}}"#;

#[test]
fn comments_and_trailing_commas_are_accepted_in_both_sections() {
    let root = tempfile::tempdir().expect("root");
    write_settings(
        root.path(),
        &format!(
            "// owner notes\n{{\n  \"providers\": {VALID_PROVIDERS}, /* inline */\n  \"ports\": {{ \"router\": 28741, }},\n}}\n"
        ),
    );

    let providers = load_or_create_provider_settings(root.path()).expect("providers");
    let ports = read_router_settings_section::<PortSettingsSection>(
        root.path(),
        RouterSettingsSection::Ports,
    )
    .expect("ports");

    assert!(!providers.claude().enabled);
    assert_eq!(
        providers.claude().executable,
        Some(PathBuf::from("/configured/claude"))
    );
    assert_eq!(
        ports.value.and_then(|ports| ports.router).map(u16::from),
        Some(28741)
    );
}

#[test]
fn broken_providers_section_fails_only_providers() {
    let root = tempfile::tempdir().expect("root");
    let path = write_settings(
        root.path(),
        r#"{"providers": {"version": 1, "providers": {}}, "ports": {"router": 28741}}"#,
    );

    let providers = load_or_create_provider_settings(root.path())
        .expect_err("an incomplete providers section must fail")
        .to_string();
    let ports = read_router_settings_section::<PortSettingsSection>(
        root.path(),
        RouterSettingsSection::Ports,
    )
    .expect("ports stay readable");

    assert!(
        providers.contains(&path.display().to_string()),
        "{providers}"
    );
    assert!(providers.contains("\"providers\" section"), "{providers}");
    assert_eq!(
        ports.value.and_then(|ports| ports.router).map(u16::from),
        Some(28741)
    );
}

#[test]
fn unsupported_providers_version_fails_the_providers_section() {
    let root = tempfile::tempdir().expect("root");
    let text = format!(
        r#"{{"providers": {}}}"#,
        VALID_PROVIDERS.replace("\"version\":1", "\"version\":2")
    );
    write_settings(root.path(), &text);

    let error = load_or_create_provider_settings(root.path())
        .expect_err("version 2 is unsupported")
        .to_string();

    assert!(error.contains("\"providers\" section"), "{error}");
    assert!(error.contains("unsupported version 2"), "{error}");
}

#[test]
fn broken_ports_section_fails_only_ports() {
    let root = tempfile::tempdir().expect("root");
    let path = write_settings(
        root.path(),
        &format!(r#"{{"providers": {VALID_PROVIDERS}, "ports": {{"router": "28741"}}}}"#),
    );

    let ports = read_router_settings_section::<PortSettingsSection>(
        root.path(),
        RouterSettingsSection::Ports,
    )
    .err()
    .expect("a string port must fail the ports section")
    .to_string();
    let providers = load_or_create_provider_settings(root.path()).expect("providers stay usable");

    assert!(ports.contains(&path.display().to_string()), "{ports}");
    assert!(ports.contains("\"ports\" section"), "{ports}");
    assert!(!providers.claude().enabled);
}

#[test]
fn file_level_faults_fail_both_sections_and_leave_the_file_untouched() {
    for (case, text) in [
        ("malformed JSONC", "{not-json".to_owned()),
        (
            "duplicate top-level member",
            r#"{"ports": {"router": 28741}, "ports": {"router": 28742}}"#.to_owned(),
        ),
        (
            "duplicate nested member",
            r#"{"ports": {"router": 28741, "router": 28742}}"#.to_owned(),
        ),
        (
            "unknown top-level member",
            format!(r#"{{"providers": {VALID_PROVIDERS}, "port": {{"router": 28741}}}}"#),
        ),
        ("top level is not an object", "[]".to_owned()),
        ("empty file", String::new()),
    ] {
        let root = tempfile::tempdir().expect("root");
        let path = write_settings(root.path(), &text);

        let providers = load_or_create_provider_settings(root.path());
        let ports = read_router_settings_section::<PortSettingsSection>(
            root.path(),
            RouterSettingsSection::Ports,
        );

        for error in [
            providers.expect_err(case).to_string(),
            ports.err().expect(case).to_string(),
        ] {
            assert!(
                error.contains("malformed Router settings"),
                "{case}: {error}"
            );
            assert!(
                error.contains(&path.display().to_string()),
                "{case}: {error}"
            );
        }
        assert_eq!(fs::read_to_string(&path).expect("kept"), text, "{case}");
    }
}

#[test]
fn absent_sections_take_defaults_without_rewriting_the_file() {
    let root = tempfile::tempdir().expect("root");
    let text = "{ /* nothing configured yet */ }\n";
    let path = write_settings(root.path(), text);

    let providers = load_or_create_provider_settings(root.path()).expect("default providers");
    let ports = read_router_settings_section::<PortSettingsSection>(
        root.path(),
        RouterSettingsSection::Ports,
    )
    .expect("ports");

    assert_eq!(providers, ProviderConfigurationEntries::default());
    assert!(ports.value.is_none());
    assert_eq!(fs::read_to_string(&path).expect("kept"), text);
}

#[test]
fn absent_file_is_created_once_with_provider_defaults_and_no_ports() {
    let root = tempfile::tempdir().expect("root");
    let path = router_settings_path(root.path());

    let created = load_or_create_provider_settings(root.path()).expect("created defaults");
    let written = fs::read_to_string(&path).expect("settings written");
    let reread = load_or_create_provider_settings(root.path()).expect("reread");
    let ports = read_router_settings_section::<PortSettingsSection>(
        root.path(),
        RouterSettingsSection::Ports,
    )
    .expect("ports");

    assert!(created.claude().enabled && created.cursor().enabled);
    assert_eq!(created.cursor().arguments, vec!["acp"]);
    assert_eq!(reread, created);
    assert!(ports.value.is_none());
    assert_eq!(fs::read_to_string(&path).expect("unchanged"), written);
}

#[test]
fn reading_ports_never_creates_the_file() {
    let root = tempfile::tempdir().expect("root");

    let ports = read_router_settings_section::<PortSettingsSection>(
        root.path(),
        RouterSettingsSection::Ports,
    )
    .expect("absent file");

    assert!(ports.value.is_none());
    assert!(!router_settings_path(root.path()).exists());
}
