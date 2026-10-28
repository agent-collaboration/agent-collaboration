//! Host-owned filesystem subscription and bounded native login reloads.
//!
//! Codex remains the only owner of OAuth refresh and auth-file writes. This
//! observer requests its existing guarded reload and retains no credentials.
use std::{convert::Infallible, future::Future, path::Path, pin::Pin, time::Duration};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::watch;

use crate::login_reload_outcome::LoginReloadOutcome;

mod native_auth_reload;

const AUTH_EVENT_DEBOUNCE: Duration = Duration::from_millis(250);
const RELOAD_DEADLINE: Duration = Duration::from_secs(10);

pub(crate) type LoginReloadFuture = Pin<Box<dyn Future<Output = Infallible> + Send>>;
pub(crate) type LoginReloadStatus = watch::Receiver<Option<LoginReloadOutcome>>;

/// Preparing before spawn closes the gap between Codex's initial read and watching.
pub(crate) async fn prepare(
    codex_home: Option<&Path>,
    socket: &Path,
) -> (LoginReloadStatus, LoginReloadFuture) {
    let (status, observed) = watch::channel(None);
    let Some(home) = codex_home else {
        return (observed, Box::pin(std::future::pending()));
    };
    let home = home.to_owned();
    let watcher = tokio::task::spawn_blocking(move || subscribe(&home)).await;
    let Ok(Ok((watcher, mut events, auth_path))) = watcher else {
        publish(
            &status,
            LoginReloadOutcome::failed("failed: auth-file watcher unavailable"),
        );
        return (observed, Box::pin(std::future::pending()));
    };
    let mut last_revision = read_revision(&auth_path)
        .await
        .ok()
        .map(|auth| auth.revision);
    let socket = socket.to_owned();
    let future = Box::pin(async move {
        // Kept inside the lifecycle-owner future, so shutdown/re-exec drops the
        // OS watch and any in-flight native exchange with no detached task.
        let _watcher = watcher;
        loop {
            if events.changed().await.is_err() {
                publish(
                    &status,
                    LoginReloadOutcome::failed("failed: auth-file watcher closed"),
                );
                std::future::pending::<()>().await;
            }
            debounce(&mut events).await;
            if *events.borrow_and_update() == FileEvent::Failed {
                publish(
                    &status,
                    LoginReloadOutcome::failed("failed: auth-file watcher event error"),
                );
                continue;
            }
            let auth = match read_revision(&auth_path).await {
                Ok(auth) => auth,
                Err(reason) => {
                    publish(&status, LoginReloadOutcome::failed(reason));
                    continue;
                }
            };
            if last_revision == Some(auth.revision) {
                continue;
            }
            // Mark the revision before dispatch: refusals/failures and duplicate
            // filesystem notifications must not retry an effectful refresh.
            last_revision = Some(auth.revision);
            let outcome =
                match tokio::time::timeout(RELOAD_DEADLINE, native_auth_reload::reload(&socket))
                    .await
                {
                    Ok(Ok(native_auth_reload::ReloadObservation::SignedOut)) => {
                        LoginReloadOutcome::RefusedFirstLogin
                    }
                    Ok(Ok(native_auth_reload::ReloadObservation::Chatgpt {
                        account_id,
                        access_digest,
                    })) => {
                        // A forced refresh may itself rotate/write the file. Suppress
                        // those queued events only after proving that Codex holds the
                        // resulting token. A concurrent unadopted revision stays queued.
                        match read_revision(&auth_path).await {
                            Ok(current)
                                if account_id
                                    .as_deref()
                                    .is_some_and(|id| id != current.account_id) =>
                            {
                                LoginReloadOutcome::RefusedDifferentAccount
                            }
                            Ok(_) if account_id.is_none() => LoginReloadOutcome::failed(
                                "failed: cached ChatGPT account identity unavailable",
                            ),
                            Ok(current) if access_digest == current.access_digest => {
                                last_revision = Some(current.revision);
                                LoginReloadOutcome::Reloaded
                            }
                            Ok(_) => LoginReloadOutcome::failed(
                                "failed: guarded reload did not adopt the changed access token",
                            ),
                            Err(reason) => LoginReloadOutcome::failed(reason),
                        }
                    }
                    Ok(Err(reason)) => LoginReloadOutcome::failed(reason),
                    Err(_) => LoginReloadOutcome::failed(
                        "failed: login reload timed out; adoption unknown",
                    ),
                };
            publish(&status, outcome);
        }
    });
    (observed, future)
}

fn publish(status: &watch::Sender<Option<LoginReloadOutcome>>, outcome: LoginReloadOutcome) {
    tracing::info!(
        login_reload = outcome.description(),
        "managed app-server login reload"
    );
    status.send_replace(Some(outcome));
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum FileEvent {
    Changed,
    Failed,
}

type Subscription = (
    RecommendedWatcher,
    watch::Receiver<FileEvent>,
    std::path::PathBuf,
);

fn subscribe(home: &Path) -> notify::Result<Subscription> {
    if RecommendedWatcher::kind() == notify::WatcherKind::PollWatcher {
        return Err(notify::Error::generic(
            "native filesystem events unavailable",
        ));
    }
    let home = home.canonicalize().map_err(notify::Error::io)?;
    let auth_path = home.join("auth.json");
    let watched_path = auth_path.clone();
    let watched_home = home.clone();
    let (events, receiver) = watch::channel(FileEvent::Changed);
    let mut watcher =
        notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
            Ok(event)
                if !matches!(event.kind, EventKind::Access(_))
                    && (event.need_rescan()
                        || event
                            .paths
                            .iter()
                            .any(|path| path == &watched_path || path == &watched_home)) =>
            {
                events.send_replace(FileEvent::Changed);
            }
            Err(_) => {
                events.send_replace(FileEvent::Failed);
            }
            Ok(_) => {}
        })?;
    // Watching the directory, rather than the auth inode, survives atomic rename.
    watcher.watch(&home, RecursiveMode::NonRecursive)?;
    Ok((watcher, receiver, auth_path))
}

async fn debounce(events: &mut watch::Receiver<FileEvent>) {
    loop {
        match tokio::time::timeout(AUTH_EVENT_DEBOUNCE, events.changed()).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) | Err(_) => return,
        }
    }
}

/// Only fingerprints and the account id survive decoding, never token strings.
struct AuthRevision {
    revision: [u8; 32],
    account_id: String,
    access_digest: [u8; 32],
}

#[derive(Deserialize)]
struct AuthFile {
    auth_mode: Option<String>,
    tokens: Option<AuthTokens>,
}

#[derive(Deserialize)]
struct AuthTokens {
    account_id: String,
    access_token: String,
}

async fn read_revision(path: &Path) -> Result<AuthRevision, &'static str> {
    // Bound reads of an owner-editable file, independently of its advertised size.
    use tokio::io::AsyncReadExt;
    let file = tokio::fs::File::open(path).await.map_err(
        |_| "failed: auth.json unavailable; logout requires explicit app-server restart",
    )?;
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| "failed: reading auth.json")?;
    if bytes.len() > 1024 * 1024 {
        return Err("failed: auth.json exceeds 1 MiB");
    }
    let document: AuthFile =
        serde_json::from_slice(&bytes).map_err(|_| "failed: invalid auth.json")?;
    if document
        .auth_mode
        .as_deref()
        .is_some_and(|mode| mode != "chatgpt")
    {
        return Err("failed: auth.json is not managed ChatGPT authentication");
    }
    let tokens = document
        .tokens
        .ok_or("failed: auth.json has no ChatGPT tokens")?;
    if tokens.account_id.is_empty() || tokens.access_token.is_empty() {
        return Err("failed: auth.json has empty ChatGPT account or access token");
    }
    Ok(AuthRevision {
        revision: Sha256::digest(&bytes).into(),
        account_id: tokens.account_id,
        access_digest: Sha256::digest(tokens.access_token.as_bytes()).into(),
    })
}
