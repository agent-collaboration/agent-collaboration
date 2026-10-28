//! Permanent real-peer lifetime and returned-error/panic reduction cases.
use super::{
    CleanupFailures, MatrixError, ScenarioCleanupFailure, run_scenario as run_matrix_scenario,
};
use crate::{FixtureResult as ProofResult, owned_peer_fixture::PeerFixture};
use collaboration_client::protocol::EndpointRef;
use futures_util::FutureExt as _;
use serde_json::json;
use sha2::{Digest as _, Sha256};
use std::{
    fmt,
    future::Ready,
    io,
    os::unix::fs::{DirBuilderExt as _, FileTypeExt as _},
    panic::{AssertUnwindSafe, resume_unwind},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{io::AsyncWriteExt as _, sync::oneshot, task::JoinHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FailureCategory {
    Scenario,
    ResourceCleanup,
    ConfigVerification,
}
#[derive(Debug)]
struct InjectedFailure {
    category: FailureCategory,
    identity: Arc<()>,
}
impl fmt::Display for InjectedFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "injected {:?} failure", self.category)
    }
}
impl std::error::Error for InjectedFailure {}
#[derive(Debug)]
struct PanicMarker {
    identity: Arc<()>,
}

fn injected(category: FailureCategory, identity: Arc<()>) -> MatrixError {
    Box::new(InjectedFailure { category, identity })
}
fn require_injected(
    error: &MatrixError,
    category: FailureCategory,
    identity: &Arc<()>,
) -> ProofResult<()> {
    let actual = error
        .downcast_ref::<InjectedFailure>()
        .ok_or("original typed error was replaced")?;
    if actual.category != category || !Arc::ptr_eq(&actual.identity, identity) {
        return Err("original error category or identity was lost".into());
    }
    Ok(())
}
fn require_panic(payload: Box<dyn std::any::Any + Send>, identity: &Arc<()>) -> ProofResult<()> {
    let actual = payload
        .downcast::<PanicMarker>()
        .map_err(|_| "original panic payload type was replaced")?;
    if !Arc::ptr_eq(&actual.identity, identity) {
        return Err("original panic payload identity was replaced".into());
    }
    Ok(())
}

#[derive(Default)]
struct Calls {
    cleanup: AtomicUsize,
    verify: AtomicUsize,
    order: Mutex<Vec<&'static str>>,
}
impl Calls {
    fn record(&self, phase: &'static str) -> ProofResult<()> {
        if phase == "cleanup" {
            self.cleanup.fetch_add(1, Ordering::SeqCst);
        }
        if phase == "verify" {
            self.verify.fetch_add(1, Ordering::SeqCst);
        }
        self.order
            .lock()
            .map_err(|_| "phase recorder poisoned")?
            .push(phase);
        Ok(())
    }
    fn require_once_in_order(&self) -> ProofResult<()> {
        if self.cleanup.load(Ordering::SeqCst) != 1 || self.verify.load(Ordering::SeqCst) != 1 {
            return Err("cleanup and verification were not each called exactly once".into());
        }
        if self
            .order
            .lock()
            .map_err(|_| "phase recorder poisoned")?
            .as_slice()
            != ["scenario", "cleanup", "verify"]
        {
            return Err("resource cleanup did not precede final verification".into());
        }
        Ok(())
    }
}

struct PrivatePeerRegistry {
    _directory: tempfile::TempDir,
    registry: PathBuf,
    socket: PathBuf,
    record: PathBuf,
    key: PathBuf,
    sentinel: PathBuf,
}
impl PrivatePeerRegistry {
    fn create() -> ProofResult<Self> {
        let directory = tempfile::Builder::new()
            .prefix("u6-finalize-")
            .tempdir_in("/tmp")?;
        let registry = directory.path().join("peer-registry");
        std::fs::DirBuilder::new().mode(0o700).create(&registry)?;
        let socket = registry.join("peer.sock");
        let pid = std::process::id();
        let record = registry.join(format!("{pid}.json"));
        let digest = Sha256::digest(socket.to_string_lossy().as_bytes());
        let key = registry.join(format!("{pid}.{digest:x}.key"));
        let sentinel = registry.join("unrelated-sentinel");
        std::fs::write(&sentinel, b"runner sentinel remains untouched")?;
        Ok(Self {
            _directory: directory,
            registry,
            socket,
            record,
            key,
            sentinel,
        })
    }
    async fn start(&self) -> ProofResult<Option<PeerFixture>> {
        let endpoint: EndpointRef = serde_json::from_value(json!({
            "serviceId":"00000000-0000-4000-8000-000000000001", "endpointId":"codex-local",
        }))?;
        let peer = PeerFixture::start_in_registry(&self.registry, &endpoint).await?;
        if let Err(error) = self.require_created() {
            let cleanup = peer.shutdown().await;
            return match cleanup {
                Ok(()) => Err(error),
                Err(cleanup) => Err(format!(
                    "peer fixture setup failed: {error}; cleanup failed: {cleanup}"
                )
                .into()),
            };
        }
        Ok(Some(peer))
    }
    fn require_created(&self) -> ProofResult<()> {
        if !std::fs::symlink_metadata(&self.socket)?
            .file_type()
            .is_socket()
            || !std::fs::symlink_metadata(&self.record)?
                .file_type()
                .is_file()
            || !std::fs::symlink_metadata(&self.key)?.file_type().is_file()
        {
            return Err("expected real peer socket, record and token file were not created".into());
        }
        let record: serde_json::Value = serde_json::from_slice(&std::fs::read(&self.record)?)?;
        if record.get("pid") != Some(&json!(std::process::id()))
            || record.get("sessionId") != Some(&json!("fixture-session"))
            || record.get("messagingSocketPath") != Some(&json!(&self.socket))
        {
            return Err("actual peer record changed its literal PID/session/socket binding".into());
        }
        let key: serde_json::Value = serde_json::from_slice(&std::fs::read(&self.key)?)?;
        if key != json!({"peerToken":"0123456789abcdef0123456789abcdef"}) {
            return Err("actual synthetic peer key differs from the independent literal".into());
        }
        Ok(())
    }
    fn require_clean(&self) -> ProofResult<()> {
        for path in [&self.socket, &self.record, &self.key] {
            match std::fs::symlink_metadata(path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
                Ok(_) => return Err("owned peer artifact remains before directory teardown".into()),
            }
        }
        if std::fs::read(&self.sentinel)? != b"runner sentinel remains untouched" {
            return Err("runner finalization changed an unrelated sentinel".into());
        }
        Ok(())
    }
}
async fn exchange_peer(owner: &mut Option<PeerFixture>, socket: &Path) -> ProofResult<()> {
    let peer = owner.as_mut().ok_or("actual peer resource missing")?;
    if serde_json::to_value(&peer.target)?
        != json!({
            "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},
            "sessionId":"fixture-session",
        })
    {
        return Err("actual peer identity changed".into());
    }
    let mut stream = tokio::net::UnixStream::connect(socket).await?;
    stream
        .write_all(b"{\"type\":\"auth\",\"token\":\"0123456789abcdef0123456789abcdef\"}\n")
        .await?;
    stream.write_all(b"{\"type\":\"user\",\"message\":{\"content\":\"runner finalization literal peer input\"}}\n").await?;
    stream.shutdown().await?;
    let received = peer
        .expect_text_with_timeout(
            "runner finalization literal peer input",
            Duration::from_secs(5),
        )
        .await?;
    if received != "runner finalization literal peer input" {
        return Err("actual peer received content differs from independent literal".into());
    }
    Ok(())
}
async fn cleanup_peer(owner: &mut Option<PeerFixture>, calls: Arc<Calls>) -> ProofResult<()> {
    calls.record("cleanup")?;
    owner
        .take()
        .ok_or("peer cleanup resource already consumed")?
        .shutdown()
        .await
}
async fn reject_unconsumed_peer(owner: &mut Option<PeerFixture>) -> ProofResult<()> {
    if let Some(peer) = owner.take() {
        let cleanup = peer.shutdown().await;
        return Err(format!(
            "finalizer skipped peer cleanup; fallback shutdown result: {cleanup:?}"
        )
        .into());
    }
    Ok(())
}

#[tokio::test]
async fn real_peer_success_joins_and_unlinks_before_verification_once() -> ProofResult<()> {
    let registry = PrivatePeerRegistry::create()?;
    let mut owner = registry.start().await?;
    let calls = Arc::new(Calls::default());
    let scenario_calls = Arc::clone(&calls);
    let cleanup_calls = Arc::clone(&calls);
    let verify_calls = Arc::clone(&calls);
    let socket = registry.socket.clone();
    let result = run_matrix_scenario(
        &mut owner,
        async move |slot: &mut &mut Option<PeerFixture>| -> ProofResult<()> {
            scenario_calls.record("scenario")?;
            exchange_peer(slot, &socket).await
        },
        move |slot| cleanup_peer(slot, cleanup_calls),
        || {
            verify_calls.record("verify")?;
            registry.require_clean()
        },
    )
    .await;
    reject_unconsumed_peer(&mut owner).await?;
    registry.require_clean()?;
    calls.require_once_in_order()?;
    result
}

#[tokio::test]
async fn real_peer_scenario_error_is_preserved_after_awaited_cleanup() -> ProofResult<()> {
    let registry = PrivatePeerRegistry::create()?;
    let mut owner = registry.start().await?;
    let calls = Arc::new(Calls::default());
    let scenario_calls = Arc::clone(&calls);
    let cleanup_calls = Arc::clone(&calls);
    let verify_calls = Arc::clone(&calls);
    let identity = Arc::new(());
    let original_identity = Arc::clone(&identity);
    let socket = registry.socket.clone();
    let result = run_matrix_scenario(
        &mut owner,
        async move |slot: &mut &mut Option<PeerFixture>| -> ProofResult<()> {
            scenario_calls.record("scenario")?;
            exchange_peer(slot, &socket).await?;
            Err(injected(FailureCategory::Scenario, identity))
        },
        move |slot| cleanup_peer(slot, cleanup_calls),
        || {
            verify_calls.record("verify")?;
            registry.require_clean()
        },
    )
    .await;
    reject_unconsumed_peer(&mut owner).await?;
    registry.require_clean()?;
    calls.require_once_in_order()?;
    let error = result.err().ok_or("scenario error became success")?;
    require_injected(&error, FailureCategory::Scenario, &original_identity)
}

#[tokio::test]
async fn real_peer_panic_resumes_original_payload_after_cleanup_and_verify() -> ProofResult<()> {
    let registry = PrivatePeerRegistry::create()?;
    let mut owner = registry.start().await?;
    let calls = Arc::new(Calls::default());
    let scenario_calls = Arc::clone(&calls);
    let cleanup_calls = Arc::clone(&calls);
    let verify_calls = Arc::clone(&calls);
    let identity = Arc::new(());
    let original_identity = Arc::clone(&identity);
    let socket = registry.socket.clone();
    let result = AssertUnwindSafe(run_matrix_scenario(
        &mut owner,
        async move |slot: &mut &mut Option<PeerFixture>| -> ProofResult<()> {
            scenario_calls.record("scenario")?;
            exchange_peer(slot, &socket).await?;
            resume_unwind(Box::new(PanicMarker { identity }))
        },
        move |slot| cleanup_peer(slot, cleanup_calls),
        || {
            verify_calls.record("verify")?;
            registry.require_clean()
        },
    ))
    .catch_unwind()
    .await;
    reject_unconsumed_peer(&mut owner).await?;
    registry.require_clean()?;
    calls.require_once_in_order()?;
    require_panic(
        result
            .err()
            .ok_or("scenario panic became a normal result")?,
        &original_identity,
    )
}

#[path = "scenario_finalization_tests/cleanup_errors.rs"]
mod cleanup_errors;
