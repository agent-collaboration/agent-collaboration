//! Standalone Serve uses the Router-root file before constructing its listener.

use super::*;

fn serve_at(root: &Path, flags: &[&str]) -> cli_argument_parsing::ServeCommand {
    let mut arguments = vec![
        OsString::from("serve"),
        OsString::from("--state-db"),
        root.join("state.sqlite").into_os_string(),
        OsString::from("--secret-root"),
        root.join("secrets").into_os_string(),
    ];
    arguments.extend(flags.iter().map(OsString::from));
    match CliCommand::parse(arguments) {
        Ok(CliCommand::Serve(command)) => command,
        other => panic!("serve command: {other:?}"),
    }
}

#[test]
fn standalone_serve_selects_the_file_port_unless_explicitly_overridden() {
    let root = tempfile::tempdir().expect("root");
    for (file, flags, expected_port) in [
        (None, vec![], 19741),
        (Some(r#"{"router":28741}"#), vec![], 28741),
        (Some(r#"{"router":28741}"#), vec!["--port", "29741"], 29741),
    ] {
        if let Some(file) = file {
            std::fs::write(root.path().join("ports.json"), file).expect("file");
        }
        let command = serve_at(root.path(), &flags);
        let actual = base_serve_runtime_config(&command).expect("runtime config");
        let expected = LoopbackRouterRuntimeConfig::new_tokenless(
            LoopbackBindAddress::new("127.0.0.1", expected_port).expect("loopback"),
            UpstreamEndpoint::new(command.upstream_base_url.clone()).expect("upstream"),
            command.state_db.clone(),
            command.secret_root.clone(),
        )
        .with_session_pin_idle_ttl(Duration::from_secs(command.session_pin_idle_ttl_seconds))
        .with_claude_five_hour_reserve_percent(command.claude_five_hour_reserve_percent);
        assert_eq!(actual, expected);
    }
}

#[test]
fn standalone_serve_refuses_a_malformed_file_with_its_path() {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("ports.json");
    std::fs::write(&path, "{").expect("file");
    let command = serve_at(root.path(), &[]);
    let message = base_serve_runtime_config(&command)
        .expect_err("a malformed port file must refuse Serve startup")
        .to_string();
    assert!(message.contains(path.to_str().expect("path")), "{message}");
}
