use super::test_support::ScriptedSessionBackend;
use super::*;
use codex_native_integration::{CodexProjectTrustLookup, ProjectTrustAnswer};
use std::{sync::Mutex, thread::ThreadId};

struct HeldModelCatalog {
    hold: std::sync::atomic::AtomicBool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
impl crate::SessionModelCatalog for HeldModelCatalog {
    fn read_models(
        &self,
        _: SessionRef,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<Vec<ProviderModelEntry>, crate::SessionModelCatalogError>,
                > + Send
                + '_,
        >,
    > {
        Box::pin(async {
            if self.hold.load(std::sync::atomic::Ordering::Acquire) {
                self.entered.notify_one();
                self.release.notified().await;
            }
            Ok(Vec::new())
        })
    }
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A held async read must not block another RPC; fixture errors propagate."
)]
async fn model_catalog_wait_does_not_block_other_native_requests()
-> Result<(), Box<dyn std::error::Error>> {
    use tokio_tungstenite::{client_async, tungstenite::Message};
    let root = tempfile::tempdir()?;
    let path = root.path().join("models.sock");
    let listener = tokio::net::UnixListener::bind(&path)?;
    let backend = ScriptedSessionBackend::new()?;
    let catalog = Arc::new(HeldModelCatalog {
        hold: false.into(),
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let context = Arc::new(RouterSessionAppServerContext::new(
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor()?,
        backend.clone(),
        backend.clone(),
        catalog.clone(),
    ));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        serve_router_session_app_server_connection(stream, context)
            .await
            .map_err(std::io::Error::other)
    });
    let (mut client, _) =
        client_async("ws://localhost/rpc", UnixStream::connect(&path).await?).await?;
    client
        .send(Message::Text(
            json!({"id":1,"method":"thread/start","params":{"cwd":root.path()}})
                .to_string()
                .into(),
        ))
        .await?;
    loop {
        let frame = client.next().await.ok_or("start response")??;
        let value: Value = serde_json::from_str(frame.to_text()?)?;
        if value.get("id") == Some(&json!(1)) {
            break;
        }
    }
    catalog
        .hold
        .store(true, std::sync::atomic::Ordering::Release);
    client
        .send(Message::Text(
            json!({"id":2,"method":"model/list","params":{}})
                .to_string()
                .into(),
        ))
        .await?;
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        catalog.entered.notified(),
    )
    .await?;
    client
        .send(Message::Text(
            json!({"id":3,"method":"initialize","params":{}})
                .to_string()
                .into(),
        ))
        .await?;
    let independent = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let frame = client.next().await.ok_or("independent RPC")??;
            let value: Value = serde_json::from_str(frame.to_text()?)?;
            if value.get("id") == Some(&json!(3)) {
                return Ok::<_, Box<dyn std::error::Error>>(value);
            }
        }
    })
    .await;
    catalog.release.notify_one();
    client.close(None).await?;
    let settlement = tokio::time::timeout(std::time::Duration::from_secs(2), server).await?;
    assert!(
        independent.is_ok(),
        "held model read blocked the native request loop"
    );
    settlement??;
    Ok(())
}

struct RecordingTrustLookup {
    observed_thread: Mutex<Option<ThreadId>>,
}

impl CodexProjectTrustLookup for RecordingTrustLookup {
    fn project_trust(&self, cwd: &Path) -> ProjectTrustAnswer {
        *self.observed_thread.lock().expect("test lock") = Some(std::thread::current().id());
        ProjectTrustAnswer::Untrusted {
            trust_target: cwd.to_string_lossy().into_owned(),
            explicitly_untrusted: false,
        }
    }
}

#[tokio::test(flavor = "current_thread")]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn config_read_runs_filesystem_trust_lookup_off_the_executor()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let backend = ScriptedSessionBackend::new()?;
    let lookup = Arc::new(RecordingTrustLookup {
        observed_thread: Mutex::new(None),
    });
    let context = Arc::new(
        RouterSessionAppServerContext::new(
            backend.endpoint.clone(),
            ScriptedSessionBackend::actor()?,
            Arc::clone(&backend) as Arc<dyn SessionCommandPort>,
            Arc::clone(&backend) as Arc<dyn SessionEventHub>,
            super::test_support::model_catalog(tokio::sync::watch::channel(Vec::new()).1),
        )
        .with_project_trust(Arc::clone(&lookup) as Arc<dyn CodexProjectTrustLookup>),
    );
    let executor_thread = std::thread::current().id();
    let reply = handle_app_server_rpc(
        context,
        json!({
            "id":1,"method":"config/read","params":{
                "includeLayers":true,"cwd":directory.path()
            }
        }),
    )
    .await?;
    assert_eq!(reply.response["id"], 1);
    let observed = lookup
        .observed_thread
        .lock()
        .expect("test lock")
        .ok_or("lookup not called")?;
    assert_ne!(observed, executor_thread);
    Ok(())
}
