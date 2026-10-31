//! Login owns the terminal only after the quota render loop and reset task settle.
use agent_router_quota_tui::AccountActionTarget;
use std::os::unix::process::ExitStatusExt;
use std::{ffi::OsString, io, path::Path, process::ExitStatus};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum QuotaLoginResult {
    Succeeded,
    Cancelled,
    Failed(String),
}

impl QuotaLoginResult {
    pub(super) fn notice(&self, label: &str) -> String {
        match self {
            Self::Succeeded => format!("Logged in again: {label}"),
            Self::Cancelled => format!("Login cancelled: {label}"),
            Self::Failed(message) => format!("Login failed for {label}: {message}"),
        }
    }
}

pub(super) async fn run_quota_login_child(
    router_root: &Path,
    target: &AccountActionTarget,
) -> QuotaLoginResult {
    // The child shares the terminal's foreground process group. Catch its Ctrl+C
    // in the parent as well; cancellation must return to quota, not kill it.
    let mut interrupt =
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()) {
            Ok(interrupt) => interrupt,
            Err(error) => return QuotaLoginResult::Failed(error.to_string()),
        };
    let mut child = match tokio::process::Command::new("codex-router")
        .args(login_arguments(router_root, target))
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return QuotaLoginResult::Failed(error.to_string()),
    };
    loop {
        tokio::select! {
            status = child.wait() => return login_result(status),
            _ = interrupt.recv() => {}
        }
    }
}

fn login_arguments(router_root: &Path, target: &AccountActionTarget) -> Vec<OsString> {
    vec![
        OsString::from("account"),
        OsString::from("login"),
        OsString::from("--provider"),
        OsString::from(target.provider.as_str()),
        OsString::from("--label"),
        OsString::from(target.label.as_str()),
        OsString::from("--router-root"),
        router_root.as_os_str().to_owned(),
    ]
}

fn login_result(status: io::Result<ExitStatus>) -> QuotaLoginResult {
    match status {
        Ok(status) if status.success() => QuotaLoginResult::Succeeded,
        Ok(status) if status.code() == Some(130) || status.signal() == Some(2) => {
            QuotaLoginResult::Cancelled
        }
        Ok(status) => QuotaLoginResult::Failed(status.to_string()),
        Err(error) => QuotaLoginResult::Failed(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_router_core::{ids::AccountId, provider::Provider};

    #[test]
    fn login_handoff_preserves_the_captured_provider_label_and_router_root() {
        for provider in [Provider::Openai, Provider::Claude] {
            let target = AccountActionTarget {
                account_id: AccountId::new("captured-id").unwrap(),
                label: agent_router_quota_tui::QuotaAccountLabel::new("label with spaces").unwrap(),
                provider,
                position: 7,
            };
            let root = Path::new("/private/tmp/quota login fixture");
            let command = crate::CliCommand::parse(login_arguments(root, &target)).unwrap();
            let crate::CliCommand::Account(crate::account::AccountCommand::Login {
                router_root,
                label,
                provider_login_flow,
                ..
            }) = command
            else {
                panic!("expected the real account login command")
            };
            assert_eq!(router_root, root);
            assert_eq!(label, "label with spaces");
            assert_eq!(
                provider_login_flow,
                match provider {
                    Provider::Openai => crate::account::ProviderLoginFlow::OpenAiDevice,
                    Provider::Claude => crate::account::ProviderLoginFlow::ClaudeOAuth,
                }
            );
        }
    }

    #[test]
    fn child_cancellation_is_distinct_from_failure_without_automatic_retry() {
        for (raw, expected) in [
            (0, QuotaLoginResult::Succeeded),
            (130 << 8, QuotaLoginResult::Cancelled),
            (2, QuotaLoginResult::Cancelled),
        ] {
            assert_eq!(login_result(Ok(ExitStatus::from_raw(raw))), expected);
        }
        assert!(matches!(
            login_result(Ok(ExitStatus::from_raw(7 << 8))),
            QuotaLoginResult::Failed(_)
        ));
        assert!(
            matches!(login_result(Err(io::Error::other("spawn refused"))), QuotaLoginResult::Failed(message) if message == "spawn refused")
        );
    }
}
