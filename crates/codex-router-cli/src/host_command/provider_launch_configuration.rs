//! Merge owner-edited provider defaults with explicit Host flags for this start.
use super::HostCommandError;
use codex_router_host::{
    ExternalProviderLaunchBinding, ExternalProviderStartup, ProviderConfigurationEntry,
    load_or_create_provider_settings, router_settings_path,
};
use collaboration_protocol::ProviderKind;
use std::path::{Path, PathBuf};

pub(super) fn configured_provider_startups(
    router_root: &Path,
    explicit_launches: &[ExternalProviderLaunchBinding],
) -> Result<Vec<ExternalProviderStartup>, HostCommandError> {
    let configuration = load_or_create_provider_settings(router_root);
    match configuration {
        Ok(configuration) => Ok(vec![
            configured_provider(
                ProviderKind::ClaudeCode,
                configuration.claude(),
                explicit_launches,
            )?,
            configured_provider(
                ProviderKind::Cursor,
                configuration.cursor(),
                explicit_launches,
            )?,
        ]),
        Err(error) => {
            let reason = error.to_string();
            let fix = format!(
                "correct the \"providers\" section of {} and restart the Host",
                router_settings_path(router_root).display()
            );
            Ok(vec![
                unavailable(ProviderKind::ClaudeCode, reason.clone(), fix.clone())?,
                unavailable(ProviderKind::Cursor, reason, fix)?,
            ])
        }
    }
}

fn configured_provider(
    provider: ProviderKind,
    entry: &ProviderConfigurationEntry,
    explicit_launches: &[ExternalProviderLaunchBinding],
) -> Result<ExternalProviderStartup, HostCommandError> {
    if let Some(explicit) = explicit_launches
        .iter()
        .find(|binding| binding.provider() == provider)
    {
        return Ok(ExternalProviderStartup::Launch(explicit.clone()));
    }
    if !entry.enabled {
        return unavailable(
            provider,
            "disabled in settings.jsonc".to_owned(),
            "enable this provider in the \"providers\" section of settings.jsonc at the router root and restart the Host"
                .to_owned(),
        );
    }
    let executable = entry.executable.clone().unwrap_or_else(|| {
        PathBuf::from(match provider {
            ProviderKind::ClaudeCode => "claude-agent-acp",
            ProviderKind::Cursor => "agent",
        })
    });
    let binding = match provider {
        ProviderKind::ClaudeCode => {
            ExternalProviderLaunchBinding::claude(executable, entry.arguments.clone())
        }
        ProviderKind::Cursor => {
            ExternalProviderLaunchBinding::cursor(executable, entry.arguments.clone())
        }
    }
    .map_err(|error| HostCommandError::RouterRoot(error.to_owned()))?;
    Ok(ExternalProviderStartup::Launch(binding))
}

fn unavailable(
    provider: ProviderKind,
    reason: String,
    fix: String,
) -> Result<ExternalProviderStartup, HostCommandError> {
    ExternalProviderStartup::unavailable(provider, reason, fix)
        .map_err(|error| HostCommandError::RouterRoot(error.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_file_selects_both_providers_without_flags() {
        let root = tempfile::tempdir().expect("router root");

        let startups = configured_provider_startups(root.path(), &[]).expect("defaults");

        assert_eq!(startups.len(), 2);
        let ExternalProviderStartup::Launch(claude) = &startups[0] else {
            panic!("Claude should launch")
        };
        assert_eq!(claude.launch.executable, PathBuf::from("claude-agent-acp"));
        let ExternalProviderStartup::Launch(cursor) = &startups[1] else {
            panic!("Cursor should launch")
        };
        assert_eq!(cursor.launch.executable, PathBuf::from("agent"));
        assert_eq!(cursor.launch.arguments, vec!["acp"]);
        assert!(root.path().join("settings.jsonc").exists());
    }

    #[test]
    fn flag_overrides_disabled_entry_for_one_start_only() {
        let root = tempfile::tempdir().expect("router root");
        let path = root.path().join("settings.jsonc");
        let bytes = br#"{
  // owner-edited providers
  "providers": {"version":1,"providers":{"claude":{"enabled":false,"executable":null,"arguments":[]},"cursor":{"enabled":true,"executable":"/configured/agent","arguments":["acp"]}}},
}"#;
        std::fs::write(&path, bytes).expect("configuration");
        let explicit = ExternalProviderLaunchBinding::claude(
            PathBuf::from("/override/claude-agent-acp"),
            vec!["--flag".to_owned()],
        )
        .expect("explicit launch");

        let startups = configured_provider_startups(root.path(), &[explicit]).expect("merged");

        let ExternalProviderStartup::Launch(claude) = &startups[0] else {
            panic!("explicit Claude should launch")
        };
        assert_eq!(
            claude.launch.executable,
            PathBuf::from("/override/claude-agent-acp")
        );
        assert_eq!(claude.launch.arguments, vec!["--flag"]);
        let ExternalProviderStartup::Launch(cursor) = &startups[1] else {
            panic!("configured Cursor should launch")
        };
        assert_eq!(cursor.launch.executable, PathBuf::from("/configured/agent"));
        assert_eq!(std::fs::read(&path).expect("unchanged file"), bytes);
    }

    #[test]
    fn malformed_file_marks_both_unavailable_without_replacement() {
        let root = tempfile::tempdir().expect("router root");
        let path = root.path().join("settings.jsonc");
        let bytes = b"{not-json";
        std::fs::write(&path, bytes).expect("malformed file");

        let startups = configured_provider_startups(root.path(), &[]).expect("isolated failure");

        for startup in startups {
            let ExternalProviderStartup::Unavailable { reason, fix, .. } = startup else {
                panic!("malformed file cannot launch a provider")
            };
            assert!(String::from(reason).contains(&path.display().to_string()));
            assert!(String::from(fix).contains("correct"));
        }
        assert_eq!(std::fs::read(&path).expect("preserved file"), bytes);
    }

    #[test]
    fn broken_providers_section_marks_both_unavailable_naming_the_section() {
        let root = tempfile::tempdir().expect("router root");
        let path = root.path().join("settings.jsonc");
        let bytes = br#"{"providers": {"version": 9}, "ports": {"router": 28741}}"#;
        std::fs::write(&path, bytes).expect("settings");

        let startups = configured_provider_startups(root.path(), &[]).expect("isolated failure");

        assert_eq!(startups.len(), 2);
        for startup in startups {
            let ExternalProviderStartup::Unavailable { reason, .. } = startup else {
                panic!("a broken providers section cannot launch a provider")
            };
            let reason = String::from(reason);
            assert!(reason.contains(&path.display().to_string()), "{reason}");
            assert!(reason.contains("\"providers\""), "{reason}");
        }
        assert_eq!(std::fs::read(&path).expect("preserved file"), bytes);
    }

    #[test]
    fn broken_ports_section_keeps_configured_providers_launchable() {
        let root = tempfile::tempdir().expect("router root");
        let bytes = br#"{"ports": {"router": "not-a-port"}, "providers": {"version":1,"providers":{"claude":{"enabled":true,"executable":"/configured/claude-agent-acp","arguments":[]},"cursor":{"enabled":true,"executable":null,"arguments":["acp"]}}}}"#;
        std::fs::write(root.path().join("settings.jsonc"), bytes).expect("settings");

        let startups = configured_provider_startups(root.path(), &[]).expect("providers");

        let ExternalProviderStartup::Launch(claude) = &startups[0] else {
            panic!("Claude should launch from its own valid section")
        };
        assert_eq!(
            claude.launch.executable,
            PathBuf::from("/configured/claude-agent-acp")
        );
    }
}
