//! Real central, enrolled machine session and public API; native capabilities are absent.
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_runtime::{CentralAutomationTransport, SessionAutomationTransport};
use collaboration_service::{ApplicationFixture, CollaborationApplication};
use std::{os::unix::fs::PermissionsExt, sync::Arc};

pub type ProofResult<Value = ()> = Result<Value, Box<dyn std::error::Error + Send + Sync>>;
pub const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
const EPOCH: &str = "00000000-0000-4000-8000-000000000002";

pub struct CatalogFixture {
    pub api: ServedCollaborationApi,
    pub application: CollaborationApplication,
    pub transport: Arc<dyn CentralAutomationTransport>,
    pub runtime: board_central_test_support::FleetTestRuntime,
    pub store: Option<Arc<tokio::sync::Mutex<automation_storage::AutomationStore>>>,
    directory: tempfile::TempDir,
}
impl CatalogFixture {
    pub async fn open(
        decorate: impl FnOnce(
            Arc<dyn CentralAutomationTransport>,
        ) -> Arc<dyn CentralAutomationTransport>,
        with_local_runs: bool,
    ) -> ProofResult<Self> {
        Self::open_with_controls(decorate, with_local_runs, Vec::new()).await
    }

    pub async fn open_with_controls(
        decorate: impl FnOnce(
            Arc<dyn CentralAutomationTransport>,
        ) -> Arc<dyn CentralAutomationTransport>,
        with_local_runs: bool,
        controls: Vec<board_central_test_support::RuntimeControlRequest>,
    ) -> ProofResult<Self> {
        let directory = tempfile::Builder::new()
            .prefix("s6cat-api-")
            .tempdir_in("/tmp")?;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
        let origin = message_board::ActorOrigin {
            installation: SERVICE.to_owned().try_into()?,
            machine: "central catalog API fixture".to_owned().try_into()?,
        };
        let central = board_central_test_support::TestCentral::start_with_peer_and_controls(
            &directory.path().join("test-central"),
            &origin,
            "019f0000-0000-7000-8000-000000001f15"
                .to_owned()
                .try_into()?,
            controls,
        )
        .await?;
        let (requests, runtime) = central.connect_machine_session(directory.path()).await?;
        let transport: Arc<dyn CentralAutomationTransport> =
            Arc::new(SessionAutomationTransport::new(requests));
        let store = if with_local_runs {
            Some(Arc::new(tokio::sync::Mutex::new(
                automation_storage::AutomationStore::open(
                    &directory.path().join("automation.sqlite"),
                )
                .await?,
            )))
        } else {
            None
        };
        let application = ApplicationFixture::new(SERVICE, EPOCH)?
            .with_catalog(Some(decorate(transport.clone())));
        let application = if let Some(store) = &store {
            application.with_automation_store(store.clone())
        } else {
            application
        }
        .build();
        let api = match ServedCollaborationApi::start(directory.path(), application.clone()) {
            Ok(api) => api,
            Err(error) => {
                runtime.shutdown().await?;
                return Err(error.into());
            }
        };
        Ok(Self {
            api,
            application,
            transport,
            runtime,
            store,
            directory,
        })
    }
    pub async fn close(self) -> ProofResult {
        let Self {
            api,
            application,
            transport,
            runtime,
            store,
            directory,
        } = self;
        let api = api.stop().await;
        let central = runtime.shutdown().await;
        drop(application);
        drop(transport);
        let storage: ProofResult = async {
            if let Some(store) = store {
                Arc::try_unwrap(store)
                    .map_err(|_| "catalog fixture Run store still owned")?
                    .into_inner()
                    .close()
                    .await?;
            }
            Ok(())
        }
        .await;
        let removed = directory.close();
        api?;
        central?;
        storage?;
        removed?;
        Ok(())
    }
}
