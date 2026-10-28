//! Startup rejects waiting/observation controls before opening any object store.
#![cfg(feature = "conformance")]

#[path = "native_http/support.rs"]
mod configuration;
use collaboration_central_conformance::{
    ConformanceResult, DispatchCallSelector, RuntimeControl, RuntimeControlRequest, fixture_policy,
};
use collaboration_central_core::ObjectAddress;
use collaboration_central_tokio::{TokioServer, TokioSettings};
use std::time::Duration;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn startup_refuses_non_arming_controls_before_discovery() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    for command in [
        RuntimeControl::Observe,
        RuntimeControl::AwaitHeldDispatch,
        RuntimeControl::ResumeDispatch,
        RuntimeControl::ResumeDispatchAndHoldNext {
            destination: ObjectAddress::Fleet,
        },
        RuntimeControl::ResumeDispatchAndHoldNextSelected {
            destination: ObjectAddress::Fleet,
            call: DispatchCallSelector::MailboxSubmit,
        },
        RuntimeControl::AwaitDroppedObjectReply,
        RuntimeControl::DropHeldDispatchReply,
        RuntimeControl::AwaitDroppedAnswer {
            frame: fleet_sync_protocol::FrameId::new(durable_steps::Uuid::now_v7())?,
        },
    ] {
        let unopened = root.path().join("must-stay-unopened");
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            TokioServer::start_with_controls(
                TokioSettings {
                    root: unopened.clone(),
                    access_policy: fixture_policy(),
                    // Rejection precedes Access transport or any object discovery.
                    jwks_url: "http://127.0.0.1:1".into(),
                    registry: configuration::registry_config()?,
                    delivery: collaboration_central_tokio::DeliveryConfig {
                        relay_peer: "019f0000-0000-7000-8000-000000005240"
                            .to_owned()
                            .try_into()?,
                    },
                    holder_answer_timeout: Duration::from_secs(2),
                    copy_ack_wait: Duration::from_secs(2),
                },
                vec![RuntimeControlRequest {
                    address: ObjectAddress::Registry,
                    command,
                }],
            ),
        )
        .await?;
        match result {
            Err(_) => {}
            Ok(server) => {
                server.shutdown().await?;
                return Err("startup accepted a waiting or observational control".into());
            }
        }
        assert!(
            !unopened.exists(),
            "invalid initial profile opened object storage"
        );
    }
    Ok(())
}
