//! Accepted debug launch options and their existing Host configuration projection.
use codex_router_host::{
    ChildCommandSpec, ChildOutput, ExternalProviderLaunchBinding, ExternalProviderStartup,
    HostConfig, HostCoordinationPaths,
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ProviderCommand {
    pub(super) executable: PathBuf,
    pub(super) arguments: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RunDirectoryAdmission {
    Fresh,
    Resume,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DebugHostOptions {
    pub(super) run_directory: PathBuf,
    pub(super) run_directory_admission: RunDirectoryAdmission,
    pub(super) router_binary: PathBuf,
    pub(super) port: u16,
    pub(super) router_root: Option<PathBuf>,
    pub(super) collaboration_directory: Option<PathBuf>,
    pub(super) claude_provider: Option<ProviderCommand>,
    pub(super) cursor_provider: Option<ProviderCommand>,
    pub(super) peer_registry_directory: Option<PathBuf>,
}

pub(super) fn parse_options() -> Result<Option<DebugHostOptions>, Box<dyn std::error::Error>> {
    parse_options_from(std::env::args_os().skip(1))
}

pub(super) fn parse_options_from(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<Option<DebugHostOptions>, Box<dyn std::error::Error>> {
    let mut arguments = arguments.into_iter();
    let mut run_directory = None::<(PathBuf, RunDirectoryAdmission)>;
    let mut router_binary = None;
    let mut port = 18787;
    let mut router_root = None;
    let mut collaboration_directory = None::<PathBuf>;
    let mut claude_executable = None;
    let mut claude_arguments = Vec::new();
    let mut cursor_executable = None;
    let mut cursor_arguments = Vec::new();
    let mut peer_registry_directory = None;
    while let Some(flag) = arguments.next() {
        if flag == "--help" || flag == "-h" {
            println!(
                "automation-debug-host (--run-directory /tmp/NEW-DIRECTORY | --resume-run-directory /tmp/OWNED-DIRECTORY) --router-binary /ABSOLUTE/target/debug/codex-router [--port 18787] [--router-root /ABSOLUTE/ISOLATED-ROOT] [--collaboration-directory /ABSOLUTE/ISOLATED-ROOT/agent-communication] [--claude-acp-executable /ABSOLUTE/ACP --claude-acp-arguments ARG ...] [--cursor-acp-executable /ABSOLUTE/ACP --cursor-acp-arguments ARG ...] [--peer-registry-directory /tmp/NEW-DIRECTORY/peer-registry]\n\nOpt-in real acceptance Host. Uses the existing codex-router-debug profile with an in-memory Luna override. Fresh mode creates runtime/workspace directories. Resume mode reopens only a stopped Host's validated private directory. Starts only its own debug router and app-server and shuts its children down on SIGTERM. Never use production paths or ports. Keep this process running while the acceptance runner connects."
            );
            return Ok(None);
        }
        let value = arguments.next().ok_or("option value missing; use --help")?;
        match flag.to_str() {
            Some("--run-directory") => select_run_directory(
                &mut run_directory,
                PathBuf::from(value),
                RunDirectoryAdmission::Fresh,
            )?,
            Some("--resume-run-directory") => select_run_directory(
                &mut run_directory,
                PathBuf::from(value),
                RunDirectoryAdmission::Resume,
            )?,
            Some("--router-binary") => router_binary = Some(PathBuf::from(value)),
            Some("--router-root") => router_root = Some(PathBuf::from(value)),
            Some("--collaboration-directory") => {
                collaboration_directory = Some(PathBuf::from(value))
            }
            Some("--peer-registry-directory") => {
                peer_registry_directory = Some(PathBuf::from(value))
            }
            Some("--claude-acp-executable") => claude_executable = Some(PathBuf::from(value)),
            Some("--claude-acp-arguments") => claude_arguments.push(
                value
                    .into_string()
                    .map_err(|_| "ACP argument is not UTF-8")?,
            ),
            Some("--cursor-acp-executable") => cursor_executable = Some(PathBuf::from(value)),
            Some("--cursor-acp-arguments") => cursor_arguments.push(
                value
                    .into_string()
                    .map_err(|_| "ACP argument is not UTF-8")?,
            ),
            Some("--port") => port = value.to_str().ok_or("port is not UTF-8")?.parse()?,
            _ => return Err("Unknown option; use --help".into()),
        }
    }
    let (run_directory, run_directory_admission) = run_directory.ok_or(
        "exactly one of --run-directory or --resume-run-directory is required; use --help",
    )?;
    if peer_registry_directory
        .as_ref()
        .is_some_and(|directory: &PathBuf| {
            !directory.is_absolute() || *directory != run_directory.join("peer-registry")
        })
    {
        return Err(
            "peer registry must be exactly the selected run directory's peer-registry child".into(),
        );
    }
    let claude_provider = match claude_executable {
        Some(executable) => {
            if !executable.is_absolute()
                || !router_root
                    .as_ref()
                    .is_some_and(|root: &PathBuf| root.is_absolute())
            {
                return Err(
                    "ACP proof requires an absolute executable and explicit absolute --router-root"
                        .into(),
                );
            }
            Some(ProviderCommand {
                executable,
                arguments: claude_arguments,
            })
        }
        None if !claude_arguments.is_empty() => {
            return Err("--claude-acp-arguments requires --claude-acp-executable".into());
        }
        None => None,
    };
    let cursor_provider = match cursor_executable {
        Some(executable) => {
            if !executable.is_absolute()
                || !router_root
                    .as_ref()
                    .is_some_and(|root: &PathBuf| root.is_absolute())
            {
                return Err(
                    "ACP proof requires an absolute executable and explicit absolute --router-root"
                        .into(),
                );
            }
            Some(ProviderCommand {
                executable,
                arguments: cursor_arguments,
            })
        }
        None if !cursor_arguments.is_empty() => {
            return Err("--cursor-acp-arguments requires --cursor-acp-executable".into());
        }
        None => None,
    };
    if router_root.as_ref().is_some_and(|root| !root.is_absolute()) {
        return Err("--router-root must be absolute".into());
    }
    if let Some(directory) = &collaboration_directory
        && !router_root.as_ref().is_some_and(|root| {
            directory.is_absolute() && *directory == root.join("agent-communication")
        })
    {
        return Err("collaboration directory requires exactly the explicit Router root agent-communication child".into());
    }
    Ok(Some(DebugHostOptions {
        run_directory,
        run_directory_admission,
        router_binary: router_binary.ok_or("--router-binary required; use --help")?,
        port,
        router_root,
        collaboration_directory,
        claude_provider,
        cursor_provider,
        peer_registry_directory,
    }))
}

fn select_run_directory(
    selected: &mut Option<(PathBuf, RunDirectoryAdmission)>,
    run_directory: PathBuf,
    admission: RunDirectoryAdmission,
) -> Result<(), &'static str> {
    if selected.is_some() {
        return Err("--run-directory and --resume-run-directory are mutually exclusive");
    }
    *selected = Some((run_directory, admission));
    Ok(())
}

impl DebugHostOptions {
    pub(super) fn selected_collaboration_directory(&self) -> PathBuf {
        self.collaboration_directory
            .clone()
            .unwrap_or_else(|| self.run_directory.join("agent-communication"))
    }

    pub(super) fn host_coordination_paths(&self, router_root: &Path) -> HostCoordinationPaths {
        // The Host derives Claude's local routing token owner from the lock parent.
        HostCoordinationPaths::new(
            self.run_directory.join("host.sock"),
            router_root.join("host.lock"),
        )
    }

    pub(super) fn resume_arguments(&self) -> Vec<OsString> {
        let mut arguments = vec![
            OsString::from("--resume-run-directory"),
            self.run_directory.as_os_str().to_owned(),
            OsString::from("--router-binary"),
            self.router_binary.as_os_str().to_owned(),
            OsString::from("--port"),
            OsString::from(self.port.to_string()),
        ];
        if let Some(root) = &self.router_root {
            arguments.extend([OsString::from("--router-root"), root.as_os_str().to_owned()]);
        }
        if let Some(directory) = &self.collaboration_directory {
            arguments.extend([
                OsString::from("--collaboration-directory"),
                directory.as_os_str().to_owned(),
            ]);
        }
        if let Some(directory) = &self.peer_registry_directory {
            arguments.extend([
                OsString::from("--peer-registry-directory"),
                directory.as_os_str().to_owned(),
            ]);
        }
        if let Some(provider) = &self.claude_provider {
            arguments.extend([
                OsString::from("--claude-acp-executable"),
                provider.executable.as_os_str().to_owned(),
            ]);
            for argument in &provider.arguments {
                arguments.extend([
                    OsString::from("--claude-acp-arguments"),
                    OsString::from(argument),
                ]);
            }
        }
        if let Some(provider) = &self.cursor_provider {
            arguments.extend([
                OsString::from("--cursor-acp-executable"),
                provider.executable.as_os_str().to_owned(),
            ]);
            for argument in &provider.arguments {
                arguments.extend([
                    OsString::from("--cursor-acp-arguments"),
                    OsString::from(argument),
                ]);
            }
        }
        arguments
    }

    pub(super) fn router_child_command(&self, router_root: &Path) -> ChildCommandSpec {
        ChildCommandSpec::new(self.router_binary.clone())
            .with_arguments([
                OsString::from("serve"),
                OsString::from("--require-debug-isolation"),
                OsString::from("--port"),
                OsString::from(self.port.to_string()),
                OsString::from("--state-db"),
                router_root.join("state.sqlite").into_os_string(),
                OsString::from("--secret-root"),
                router_root.join("secrets").into_os_string(),
                OsString::from("--audit-file"),
                self.run_directory
                    .join("router-audit.jsonl")
                    .into_os_string(),
            ])
            .with_output(ChildOutput::Telemetry)
    }

    pub(super) fn attach_provider(
        &self,
        config: HostConfig,
        global_claude_executable: Option<PathBuf>,
    ) -> Result<HostConfig, Box<dyn std::error::Error>> {
        let mut config = config;
        if let Some(provider) = &self.claude_provider {
            let mut binding = ExternalProviderLaunchBinding::claude(
                provider.executable.clone(),
                provider.arguments.clone(),
            )?;
            let executable = global_claude_executable
                .ok_or("ACP proof requires the existing global CLAUDE_CODE_EXECUTABLE selector")?;
            if !executable.is_absolute() {
                return Err("global Claude selector must be absolute".into());
            }
            let executable = executable
                .to_str()
                .ok_or("global Claude selector is not UTF-8")?
                .to_owned();
            binding
                .launch
                .environment
                .push(("CLAUDE_CODE_EXECUTABLE".to_owned(), executable));
            config =
                config.with_external_provider_startup(ExternalProviderStartup::Launch(binding));
        }
        if let Some(provider) = &self.cursor_provider {
            let binding = ExternalProviderLaunchBinding::cursor(
                provider.executable.clone(),
                provider.arguments.clone(),
            )?;
            config =
                config.with_external_provider_startup(ExternalProviderStartup::Launch(binding));
        }
        Ok(config)
    }
}

#[cfg(test)]
mod collaboration_tests {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    #[test]
    fn public_discovery_directory_preserves_router_token_ownership()
    -> Result<(), Box<dyn std::error::Error>> {
        let run = tempfile::tempdir()?;
        let root = tempfile::tempdir()?;
        let public_service = root.path().join("agent-communication");
        // The old RUN projection points discovery at a different token owner.
        assert_ne!(
            run.path().join("agent-communication").parent(),
            public_service.parent()
        );
        let options = parse_options_from([
            OsString::from("--run-directory"),
            run.path().as_os_str().to_owned(),
            OsString::from("--router-binary"),
            OsString::from("/tmp/router"),
            OsString::from("--router-root"),
            root.path().as_os_str().to_owned(),
            OsString::from("--collaboration-directory"),
            public_service.as_os_str().to_owned(),
        ])?
        .ok_or("launch options absent")?;
        assert_eq!(options.selected_collaboration_directory(), public_service);
        let socket_arguments = super::super::proof_permissions::automation_proof_startup_arguments(
            &options.selected_collaboration_directory(),
        )?;
        assert!(socket_arguments.iter().any(|arg| {
            arg.to_string_lossy().contains(
                root.path()
                    .canonicalize()
                    .unwrap()
                    .join("agent-communication/control.sock")
                    .to_str()
                    .unwrap(),
            )
        }));
        let resumed = parse_options_from(options.resume_arguments())?.ok_or("resume absent")?;
        let context = super::super::DebugHostContext {
            kind: "debugHostPrepared".to_owned(),
            run_directory: options.run_directory.clone(),
            service_directory: public_service,
            workspace: run.path().join("agent-workspace"),
            profile: "codex-router-debug".to_owned(),
            model: "gpt-5.6-luna".to_owned(),
            port: options.port,
            host_pid: std::process::id(),
            router_root: options.router_root.clone(),
            claude_provider: None,
            cursor_provider: None,
            peer_registry_directory: None,
        };
        super::super::validate_resume_binding(&context, &resumed)?;
        let mut removed = resumed.clone();
        removed.collaboration_directory = None;
        assert!(super::super::validate_resume_binding(&context, &removed).is_err());
        assert_eq!(
            resumed,
            DebugHostOptions {
                run_directory_admission: RunDirectoryAdmission::Resume,
                ..options
            }
        );
        Ok(())
    }

    #[test]
    fn public_directory_requires_explicit_canonical_private_root_without_publication()
    -> Result<(), Box<dyn std::error::Error>> {
        let run = tempfile::tempdir()?;
        let root = tempfile::tempdir()?;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
        let root_path = root.path().canonicalize()?;
        let service = root_path.join("agent-communication");
        let mut options = parse_options_from([
            OsString::from("--run-directory"),
            run.path().as_os_str().to_owned(),
            OsString::from("--router-binary"),
            OsString::from("/tmp/router"),
            OsString::from("--router-root"),
            root_path.as_os_str().to_owned(),
            OsString::from("--collaboration-directory"),
            service.as_os_str().to_owned(),
        ])?
        .ok_or("options absent")?;
        super::super::validate_collaboration_directory(&options)?;
        std::fs::DirBuilder::new().mode(0o700).create(&service)?;
        super::super::validate_collaboration_directory(&options)?;
        std::fs::write(service.join("service.json"), b"existing publication")?;
        assert!(super::super::validate_collaboration_directory(&options).is_err());
        std::fs::remove_file(service.join("service.json"))?;
        std::fs::set_permissions(&service, std::fs::Permissions::from_mode(0o755))?;
        assert!(super::super::validate_collaboration_directory(&options).is_err());
        std::fs::set_permissions(&service, std::fs::Permissions::from_mode(0o700))?;
        let alias = root_path.join("foreign-service");
        std::os::unix::fs::symlink(&service, &alias)?;
        options.collaboration_directory = Some(alias);
        assert!(super::super::validate_collaboration_directory(&options).is_err());
        let other_root = tempfile::tempdir()?;
        std::fs::set_permissions(other_root.path(), std::fs::Permissions::from_mode(0o700))?;
        let other_path = other_root.path().canonicalize()?;
        let linked_service = other_path.join("agent-communication");
        std::os::unix::fs::symlink(&service, &linked_service)?;
        options.router_root = Some(other_path);
        options.collaboration_directory = Some(linked_service);
        assert!(super::super::validate_collaboration_directory(&options).is_err());
        options.collaboration_directory = Some(service);
        options.router_root = Some(root_path.join("."));
        assert!(super::super::validate_collaboration_directory(&options).is_err());
        Ok(())
    }

    #[test]
    fn default_run_projection_and_invalid_public_directory_arguments_are_preserved() {
        let base = [
            "--run-directory",
            "/tmp/run",
            "--router-binary",
            "/tmp/router",
        ];
        let options = parse_options_from(base.map(OsString::from))
            .unwrap()
            .unwrap();
        assert_eq!(
            options.selected_collaboration_directory(),
            PathBuf::from("/tmp/run/agent-communication")
        );
        assert!(
            !options
                .resume_arguments()
                .contains(&OsString::from("--collaboration-directory"))
        );
        for extras in [
            vec!["--collaboration-directory", "/tmp/root/agent-communication"],
            vec![
                "--router-root",
                "/tmp/root",
                "--collaboration-directory",
                "/tmp/foreign/agent-communication",
            ],
            vec![
                "--router-root",
                "/tmp/root",
                "--collaboration-directory",
                "relative",
            ],
        ] {
            assert!(
                parse_options_from(base.into_iter().chain(extras).map(OsString::from)).is_err()
            );
        }
    }
}
