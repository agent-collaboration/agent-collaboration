//! Owner port choices survive replacement without being promoted into flags.

use super::foreground_launch::{
    HostLaunchMode, HostReplacementCommandInputs, host_replacement_command,
};
use super::{HostCommand, run_host_command};
use crate::CliContext;
use codex_router_host::{ChildCommandSpec, HostPortSource, RemoteAccess};
use codex_router_host::{HostPortOverrides, resolve_host_ports};
use std::{
    ffi::OsString,
    net::SocketAddr,
    num::NonZeroU16,
    path::{Path, PathBuf},
};

fn parse_host(flags: &[&str]) -> HostCommand {
    HostCommand::parse(flags.iter().map(OsString::from).collect()).expect("Host command")
}

#[test]
fn isolated_ports_refuse_every_protected_flag() {
    assert_protected_ports_refused(false);
}

#[test]
fn isolated_ports_refuse_every_protected_file_field() {
    assert_protected_ports_refused(true);
}

#[derive(Clone, Copy)]
enum PortField {
    Router,
    Mcp,
    TailscaleHttps,
}

fn assert_protected_ports_refused(from_file: bool) {
    let root = tempfile::tempdir().expect("root");
    for (field, file_key) in [
        (PortField::Router, "router"),
        (PortField::Mcp, "mcp"),
        (PortField::TailscaleHttps, "tailscaleHttps"),
    ] {
        for port in [19741, 19742, 19743] {
            let mut overrides = HostPortOverrides {
                router: Some(28741),
                mcp_bind: Some("127.0.0.1:28742".parse().expect("loopback")),
                tailscale_https: NonZeroU16::new(28743),
            };
            if from_file {
                write_ports_section(root.path(), &format!("{{\"{file_key}\":{port}}}"));
                match field {
                    PortField::Router => overrides.router = None,
                    PortField::Mcp => overrides.mcp_bind = None,
                    PortField::TailscaleHttps => overrides.tailscale_https = None,
                }
            } else {
                match field {
                    PortField::Router => overrides.router = Some(port),
                    PortField::Mcp => overrides.mcp_bind = Some(([127, 0, 0, 1], port).into()),
                    PortField::TailscaleHttps => overrides.tailscale_https = NonZeroU16::new(port),
                }
            }
            let error = resolve_host_ports(root.path(), true, overrides)
                .expect_err("isolated resolution must reject the production port before bind");
            let message = error.to_string();
            assert!(message.contains(&port.to_string()), "{file_key}: {message}");
            assert!(message.contains("settings.jsonc"), "{file_key}: {message}");
            assert!(message.contains("\"ports\""), "{file_key}: {message}");
        }
    }
}

#[test]
fn replacement_command_omits_resolved_ports_without_launch_flags() {
    // Arrange both default and file-derived choices, with no explicit launch flags.
    let root = tempfile::tempdir().expect("root");
    for file in [
        None,
        Some(r#"{"router":28741,"mcp":28742,"tailscaleHttps":28743}"#),
    ] {
        if let Some(bytes) = file {
            write_ports_section(root.path(), bytes);
        }
        let ports =
            resolve_host_ports(root.path(), true, HostPortOverrides::default()).expect("ports");
        // Act through the real replacement-command projection.
        let command = host_replacement_command(HostReplacementCommandInputs {
            executable: PathBuf::from("/tmp/codex-router"),
            router_root: root.path().to_owned(),
            port_overrides: HostPortOverrides::default(),
            remote_access: Some(RemoteAccess::tailscale(ports.tailscale_https_port())),
            provider_operation_retention_days: Some(std::num::NonZeroU32::new(60).expect("days")),
            owner_human_id: None,
            external_provider_launches: &[],
            launch_mode: HostLaunchMode::IsolatedDebug,
        });
        // Assert: no Router, MCP, or Tailscale port may become an explicit flag.
        assert_eq!(
            command,
            ChildCommandSpec::new(PathBuf::from("/tmp/codex-router")).with_arguments([
                OsString::from("host"),
                OsString::from("--router-root"),
                root.path().into(),
                OsString::from("--provider-operation-retention-days"),
                OsString::from("60"),
                OsString::from("--remote-access"),
                OsString::from("tailscale"),
                OsString::from("--require-debug-isolation")
            ])
        );
    }
}

#[test]
fn replacement_command_keeps_only_operator_explicit_ports() {
    let root = tempfile::tempdir().expect("root");
    write_ports_section(
        root.path(),
        r#"{"router":28741,"mcp":28742,"tailscaleHttps":28743}"#,
    );
    for flags in [
        vec!["--port", "29741"],
        vec!["--mcp-bind", "[::1]:29742"],
        vec![
            "--remote-access",
            "tailscale",
            "--tailscale-https-port",
            "29743",
        ],
    ] {
        let parsed = parse_host(&flags);
        let overrides = parsed.port_overrides();
        let ports = resolve_host_ports(root.path(), true, overrides).expect("ports");
        let command = host_replacement_command(HostReplacementCommandInputs {
            executable: PathBuf::from("/tmp/codex-router"),
            router_root: root.path().to_owned(),
            port_overrides: overrides,
            remote_access: Some(RemoteAccess::tailscale(ports.tailscale_https_port())),
            provider_operation_retention_days: Some(std::num::NonZeroU32::new(60).expect("days")),
            owner_human_id: None,
            external_provider_launches: &[],
            launch_mode: HostLaunchMode::IsolatedDebug,
        });
        let mut expected = vec![
            OsString::from("host"),
            OsString::from("--router-root"),
            root.path().into(),
        ];
        for (flag, value) in [
            ("--port", overrides.router.map(|port| port.to_string())),
            (
                "--mcp-bind",
                overrides.mcp_bind.map(|bind| bind.to_string()),
            ),
        ] {
            if let Some(value) = value {
                expected.extend([OsString::from(flag), OsString::from(value)]);
            }
        }
        expected.extend(
            [
                "--provider-operation-retention-days",
                "60",
                "--remote-access",
                "tailscale",
            ]
            .map(OsString::from),
        );
        if overrides.tailscale_https.is_some() {
            expected.extend(["--tailscale-https-port", "29743"].map(OsString::from));
        }
        expected.push(OsString::from("--require-debug-isolation"));
        assert_eq!(
            command,
            ChildCommandSpec::new(PathBuf::from("/tmp/codex-router")).with_arguments(expected)
        );
    }
}

async fn assert_invalid_startup(ports_section: &str) {
    let root = tempfile::tempdir().expect("root");
    let path = write_ports_section(root.path(), ports_section);
    assert_startup_refused_naming(root.path(), &path, "\"ports\"").await;
}

async fn assert_startup_refused_naming(root: &Path, path: &Path, section_or_file: &str) {
    let bytes = std::fs::read_to_string(path).expect("settings fixture");
    let command = HostCommand::parse(vec![OsString::from("--router-root"), root.into()])
        .expect("Host command");
    let context = CliContext::new(vec![("HOME".to_owned(), root.display().to_string())]);
    let error = run_host_command(&mut Vec::new(), command, &context, None)
        .await
        .expect_err("invalid configuration must refuse startup")
        .to_string();
    assert!(
        error.contains(&path.display().to_string()),
        "{bytes}: {error}"
    );
    assert!(error.contains(section_or_file), "{bytes}: {error}");
}

#[tokio::test]
async fn unknown_ports_field_refuses_startup_with_path() {
    assert_invalid_startup(r#"{"unknown":1}"#).await;
}
#[tokio::test]
async fn non_loopback_mcp_refuses_startup_with_path() {
    assert_invalid_startup(r#"{"mcp":"192.0.2.1:28742"}"#).await;
}
#[tokio::test]
async fn zero_file_ports_refuse_startup_with_path() {
    for bytes in [r#"{"router":0}"#, r#"{"mcp":0}"#, r#"{"tailscaleHttps":0}"#] {
        assert_invalid_startup(bytes).await;
    }
}
#[tokio::test]
async fn equal_file_ports_refuse_startup_with_path() {
    assert_invalid_startup(r#"{"router":28741,"mcp":28741}"#).await;
}
#[tokio::test]
async fn malformed_ports_section_refuses_startup_with_path() {
    assert_invalid_startup("[1, 2]").await;
}
#[tokio::test]
async fn malformed_settings_file_refuses_startup_with_path() {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("settings.jsonc");
    std::fs::write(&path, "{not-json").expect("malformed settings");
    assert_startup_refused_naming(root.path(), &path, "settings.jsonc").await;
}

#[test]
fn unreadable_settings_file_refuses_with_path() {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("settings.jsonc");
    std::fs::create_dir(&path).expect("unreadable directory");
    let error = resolve_host_ports(root.path(), true, HostPortOverrides::default())
        .expect_err("cannot read a directory")
        .to_string();
    assert!(error.contains(&path.display().to_string()), "{error}");
}

#[test]
fn port_resolution_table_for_normal_and_debug_host() {
    use codex_router_host::HostPortSource::{Default, Flag, SettingsPorts};
    for isolated in [false, true] {
        let root = tempfile::tempdir().expect("root");
        let expected_defaults = if isolated {
            (18787, 18788)
        } else {
            (19741, 19742)
        };
        let defaults = resolve_host_ports(root.path(), isolated, HostPortOverrides::default())
            .expect("defaults");
        assert_eq!(
            (
                defaults.selection().router.port,
                defaults.selection().mcp.port
            ),
            expected_defaults
        );
        assert_eq!(
            (
                defaults.selection().router.source,
                defaults.selection().mcp.source
            ),
            (Default, Default)
        );
        write_ports_section(root.path(), r#"{"router":28741,"mcp":28742}"#);
        for (flags, expected_ports, expected_sources) in [
            (vec![], (28741, 28742), (SettingsPorts, SettingsPorts)),
            (
                vec!["--port", "28751"],
                (28751, 28742),
                (Flag, SettingsPorts),
            ),
            (
                vec!["--mcp-bind", "[::1]:28752"],
                (28741, 28752),
                (SettingsPorts, Flag),
            ),
            (
                vec!["--port", "28751", "--mcp-bind", "[::1]:28752"],
                (28751, 28752),
                (Flag, Flag),
            ),
        ] {
            let selected =
                resolve_host_ports(root.path(), isolated, parse_host(&flags).port_overrides())
                    .expect("selected");
            assert_eq!(
                (
                    selected.selection().router.port,
                    selected.selection().mcp.port
                ),
                expected_ports
            );
            assert_eq!(
                (
                    selected.selection().router.source,
                    selected.selection().mcp.source
                ),
                expected_sources
            );
            if expected_sources.1 == Flag {
                assert_eq!(
                    selected.mcp_bind(),
                    "[::1]:28752".parse::<SocketAddr>().expect("IPv6")
                );
            }
        }
    }
}

#[test]
fn partial_file_defaults_and_resolved_conflicts_are_validated() {
    let root = tempfile::tempdir().expect("root");
    for (bytes, expected) in [
        (r#"{"router":28741}"#, (28741, 18788)),
        (r#"{"mcp":28742}"#, (18787, 28742)),
    ] {
        write_ports_section(root.path(), bytes);
        let ports = resolve_host_ports(root.path(), true, HostPortOverrides::default())
            .expect("partial configuration");
        assert_eq!(
            (ports.selection().router.port, ports.selection().mcp.port),
            expected
        );
    }
    let path = write_ports_section(root.path(), r#"{"mcp":18787}"#);
    let error = resolve_host_ports(root.path(), true, HostPortOverrides::default())
        .expect_err("resolved ports conflict")
        .to_string();
    assert!(error.contains(&path.display().to_string()));
}

#[test]
fn tailscale_resolution_uses_the_named_normal_and_debug_defaults_and_file_override() {
    for (isolated, default_port) in [(false, 19743), (true, 18789)] {
        let root = tempfile::tempdir().expect("root");
        let defaults = resolve_host_ports(root.path(), isolated, HostPortOverrides::default())
            .expect("defaults");
        assert_eq!(defaults.tailscale_https_port().get(), default_port);
        assert_eq!(
            defaults.selection().tailscale_https.source,
            HostPortSource::Default
        );
        write_ports_section(root.path(), r#"{"tailscaleHttps":29743}"#);
        let file =
            resolve_host_ports(root.path(), isolated, HostPortOverrides::default()).expect("file");
        assert_eq!(file.tailscale_https_port().get(), 29743);
        assert_eq!(
            file.selection().tailscale_https.source,
            HostPortSource::SettingsPorts
        );
        let overrides = parse_host(&[
            "--remote-access",
            "tailscale",
            "--tailscale-https-port",
            "29744",
        ])
        .port_overrides();
        let flag = resolve_host_ports(root.path(), isolated, overrides).expect("flag");
        assert_eq!(
            flag.tailscale_https_port(),
            NonZeroU16::new(29744).expect("port")
        );
        assert_eq!(
            flag.selection().tailscale_https.source,
            HostPortSource::Flag
        );
    }
}

/// Writes `settings.jsonc` with only a `ports` section holding `ports_section` verbatim.
fn write_ports_section(root: &Path, ports_section: &str) -> PathBuf {
    let path = root.join("settings.jsonc");
    std::fs::write(
        &path,
        format!("{{\n  // owner-edited Host ports\n  \"ports\": {ports_section},\n}}\n"),
    )
    .expect("settings fixture");
    path
}

#[test]
fn broken_providers_section_leaves_ports_resolvable() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(
        root.path().join("settings.jsonc"),
        r#"{"providers": {"version": 9}, "ports": {"router": 28741, "mcp": 28742}}"#,
    )
    .expect("settings fixture");

    let ports = resolve_host_ports(root.path(), true, HostPortOverrides::default())
        .expect("a broken providers section must not stop port resolution");

    assert_eq!(
        (ports.selection().router.port, ports.selection().mcp.port),
        (28741, 28742)
    );
}

#[test]
fn legacy_ports_file_is_not_read() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("ports.json"), r#"{"router":28741}"#).expect("legacy file");

    let ports =
        resolve_host_ports(root.path(), true, HostPortOverrides::default()).expect("defaults");

    assert_eq!(ports.selection().router.port, 18787);
}
