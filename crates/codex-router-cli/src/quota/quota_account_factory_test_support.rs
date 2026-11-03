//! An owned fake-root capability scopes the existing account factory during CLI login restarts.
use agent_router_quota_tui::QuotaAccountCommands;
use std::{future::Future, io, path::Path};

pub(crate) struct OwnedQuotaFixtureRoot(tempfile::TempDir);
impl OwnedQuotaFixtureRoot {
    pub(crate) fn create() -> io::Result<Self> {
        tempfile::Builder::new()
            .prefix("codex-router-quota-effect-")
            .tempdir()
            .map(Self)
    }

    pub(crate) fn path(&self) -> &Path {
        self.0.path()
    }

    pub(crate) async fn with_commands<T>(
        &self,
        commands: QuotaAccountCommands,
        future: impl Future<Output = T>,
    ) -> io::Result<T> {
        let binding = AccountFactoryBinding {
            root: self.path().canonicalize()?,
            commands,
        };
        Ok(ACTIVE_FIXTURE_FACTORY.scope(binding, future).await)
    }
}

struct AccountFactoryBinding {
    root: std::path::PathBuf,
    commands: QuotaAccountCommands,
}
tokio::task_local! {
    static ACTIVE_FIXTURE_FACTORY: AccountFactoryBinding;
}

pub(crate) fn commands_for(router_root: &Path) -> Option<QuotaAccountCommands> {
    ACTIVE_FIXTURE_FACTORY
        .try_with(|binding| {
            let requested = router_root.canonicalize().ok();
            if requested.as_deref() != Some(binding.root.as_path()) {
                let rejected: QuotaAccountCommands = std::sync::Arc::new(|_| {
                    Box::pin(async {
                        super::failed(
                            "Quota fixture account factory rejected a different root".to_owned(),
                        )
                    })
                });
                return rejected;
            }
            binding.commands.clone()
        })
        .ok()
}
