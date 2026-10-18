//! Isolated Host profile loading; owner launches never read the debug profile.

use std::path::Path;

use codex_native_integration::DebugCodexProfile;

use super::{HostCommandError, HostLaunchMode};

pub(super) fn load_debug_profile(
    codex_home: &Path,
    launch_mode: HostLaunchMode,
    port: u16,
) -> Result<Option<DebugCodexProfile>, HostCommandError> {
    if !launch_mode.is_isolated() {
        return Ok(None);
    }
    let path = codex_home.join("codex-router-debug.config.toml");
    #[cfg(debug_assertions)]
    let profile = DebugCodexProfile::read_for_isolated_launch(codex_home, port);
    #[cfg(not(debug_assertions))]
    let profile = DebugCodexProfile::read(codex_home, port);
    let profile =
        profile.map_err(|source| HostCommandError::IsolatedDebugProfile { path, source })?;
    Ok(Some(profile))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(debug_assertions)]
    use codex_native_integration::{AppServerCommandSpec, CodexPaths, CodexRouterProfile};

    const SAVED_PROFILE: &str = r#"
model = "saved-model"
model_reasoning_effort = "medium"
model_reasoning_summary = "detailed"
model_verbosity = "low"
model_provider = "codex-router-debug"
[projects."/work/fixture"]
trust_level = "trusted"
[model_providers.codex-router-debug]
name = "OpenAI"
base_url = "http://127.0.0.1:18787/v1"
wire_api = "responses"
requires_openai_auth = true
supports_websockets = true
stream_max_retries = 2
[features]
enable_request_compression = false
"#;

    #[cfg(debug_assertions)]
    #[test]
    fn isolated_debug_profile_projects_only_endpoint_and_preserves_saved_bytes() {
        // Arrange: the real saved profile and Host intentionally select different ports.
        let directory = tempfile::tempdir().unwrap();
        let profile_path = directory.path().join("codex-router-debug.config.toml");
        std::fs::write(&profile_path, SAVED_PROFILE).unwrap();
        let saved = DebugCodexProfile::read(directory.path(), 18787).unwrap();
        let paths = CodexPaths::from_codex_home(directory.path().to_owned());
        let command = AppServerCommandSpec::new(
            &paths,
            &CodexRouterProfile::new(28787),
            &directory.path().join("app-server.sock"),
        );
        let expected = command.clone().with_debug_profile(&saved);

        // Act: call the same loader as the foreground Host, with actual file I/O.
        let loaded = load_debug_profile(directory.path(), HostLaunchMode::IsolatedDebug, 28787)
            .unwrap()
            .unwrap();
        let actual = command.with_debug_profile(&loaded);

        // Assert: exact native argv and environment, with only the provider endpoint replaced.
        let expected_arguments: Vec<std::ffi::OsString> = expected
            .arguments()
            .iter()
            .map(|argument| {
                let text = argument.to_str().unwrap();
                if text.starts_with("model_providers=") {
                    text.replace("http://127.0.0.1:18787/v1", "http://127.0.0.1:28787/v1")
                        .into()
                } else {
                    argument.clone()
                }
            })
            .collect();
        assert_eq!(actual.arguments(), expected_arguments);
        assert_ne!(actual.arguments(), expected.arguments());
        assert_eq!(actual.environment(), expected.environment());
        assert_eq!(
            std::fs::read(&profile_path).unwrap(),
            SAVED_PROFILE.as_bytes()
        );
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn non_debug_isolated_launch_keeps_strict_saved_port_routing() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("codex-router-debug.config.toml"),
            SAVED_PROFILE,
        )
        .unwrap();
        assert!(
            load_debug_profile(directory.path(), HostLaunchMode::IsolatedDebug, 18787)
                .unwrap()
                .is_some()
        );
        assert!(matches!(
            load_debug_profile(directory.path(), HostLaunchMode::IsolatedDebug, 28787),
            Err(HostCommandError::IsolatedDebugProfile {
                source: codex_native_integration::DebugProfileError::RoutingMismatch,
                ..
            })
        ));
    }

    #[test]
    fn isolated_debug_profile_refuses_invalid_saved_settings_and_reserved_ports() {
        let directory = tempfile::tempdir().unwrap();
        let profile_path = directory.path().join("codex-router-debug.config.toml");
        for invalid in [
            SAVED_PROFILE.replace("18787/v1", "19741/v1"),
            SAVED_PROFILE.replace("127.0.0.1", "example.invalid"),
            SAVED_PROFILE.replace("name = \"OpenAI\"", "name = \"other\""),
            format!("{SAVED_PROFILE}\nunsupported = true\n"),
        ] {
            std::fs::write(&profile_path, &invalid).unwrap();
            assert!(
                load_debug_profile(directory.path(), HostLaunchMode::IsolatedDebug, 28787).is_err()
            );
            assert_eq!(std::fs::read(&profile_path).unwrap(), invalid.as_bytes());
        }
        std::fs::write(&profile_path, SAVED_PROFILE).unwrap();
        for port in [0, codex_native_integration::DEFAULT_ROUTER_PORT] {
            assert!(
                load_debug_profile(directory.path(), HostLaunchMode::IsolatedDebug, port).is_err()
            );
        }
    }

    #[test]
    fn owner_launch_does_not_read_or_validate_debug_profile() {
        // An attempted read of this missing home would fail, even before profile parsing.
        let directory = tempfile::tempdir().unwrap();
        let absent_home = directory.path().join("absent-home");
        assert!(
            load_debug_profile(&absent_home, HostLaunchMode::OwnerProduction, 19741)
                .unwrap()
                .is_none()
        );
        assert!(!absent_home.exists());
    }
}
