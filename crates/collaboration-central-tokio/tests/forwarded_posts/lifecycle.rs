use super::*;
use collaboration_central_conformance::{
    RuntimeControl, RuntimeControlRequest, RuntimeFaultObservation, owner_assertion,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};

use super::forwarded_post_lifecycle_oracles::{
    self as lifecycle_oracle, POST_MARKER, answer, holder_frame, outcome, post,
};

async fn control(
    server: &collaboration_central_tokio::TokioServer,
    command: RuntimeControl,
) -> ConformanceResult<RuntimeFaultObservation> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(10))
        .build()?
        .post(format!(
            "{}/admin/v1/conformance/runtime",
            server.base_url()
        ))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&RuntimeControlRequest {
            address: ObjectAddress::Connection(ServiceId::try_from(SENDER_SERVICE.to_owned())?),
            command,
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
#[tokio::test]
async fn the_sixteenth_slot_is_reserved_before_registry_dispatch_and_a_terminal_frees_it()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let database = arrange_holder(root.path(), &jwks.url).await?;
    let server = start(root.path(), &jwks.url).await?;
    let proof = async {
        let mut sender = connect(&server, SENDER_CREDENTIAL).await?;
        let mut holder = connect(&server, HOLDER_CREDENTIAL).await?;
        lifecycle_oracle::capacity(&mut sender, &mut holder, database, |command| {
            control(&server, command)
        })
        .await?;
        close(sender).await?;
        close(holder).await?;
        Ok(())
    }
    .await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    proof
}

#[tokio::test]
async fn the_original_deadline_times_out_once_and_a_late_holder_reply_cannot_answer_a_new_request()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let database = arrange_holder(root.path(), &jwks.url).await?;
    let server = start(root.path(), &jwks.url).await?;
    let proof = async {
        let mut sender = connect(&server, SENDER_CREDENTIAL).await?;
        let mut holder = connect(&server, HOLDER_CREDENTIAL).await?;
        lifecycle_oracle::deadline(&mut sender, &mut holder, database, |command| {
            control(&server, command)
        })
        .await?;
        close(sender).await?;
        close(holder).await?;
        Ok(())
    }
    .await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    proof
}

#[tokio::test]
async fn reopening_preserves_correlation_slots_and_deadlines_without_persisting_the_opaque_post()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let database = arrange_holder(root.path(), &jwks.url).await?;
    let server = start(root.path(), &jwks.url).await?;
    let started = std::time::SystemTime::now();
    let arranged = async {
        let mut sender = connect(&server, SENDER_CREDENTIAL).await?;
        let mut holder = connect(&server, HOLDER_CREDENTIAL).await?;
        control(
            &server,
            RuntimeControl::SetTime {
                now: RegistryTime(started.into()),
            },
        )
        .await?;
        let mut frames = Vec::new();
        for _ in 0..16 {
            let request = post(&mut sender, database).await?;
            let _delivered = holder_frame(&mut holder).await?;
            frames.push(request);
        }
        close(sender).await?;
        close(holder).await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(frames)
    }
    .await;
    server.shutdown().await?;
    let frames = arranged?;
    let mut observed_identity = false;
    let mut paths = vec![root.path().to_path_buf()];
    while let Some(path) = paths.pop() {
        if path.is_dir() {
            for entry in std::fs::read_dir(path)? {
                paths.push(entry?.path());
            }
        } else {
            let bytes = std::fs::read(path)?;
            assert!(
                !bytes
                    .windows(POST_MARKER.len())
                    .any(|part| part == POST_MARKER.as_bytes()),
                "opaque forwarded content entered the reopened runtime storage"
            );
            let identity = frames[0].to_string();
            observed_identity |= bytes
                .windows(identity.len())
                .any(|part| part == identity.as_bytes());
        }
    }
    assert!(
        observed_identity,
        "persistence scan did not reach the real pending correlation rows"
    );

    let reopened = start(root.path(), &jwks.url).await?;
    let proof = async {
        let mut sender = connect(&reopened, SENDER_CREDENTIAL).await?;
        let mut holder = connect(&reopened, HOLDER_CREDENTIAL).await?;
        control(
            &reopened,
            RuntimeControl::SetTime {
                now: RegistryTime(started.into()),
            },
        )
        .await?;
        let overflow = post(&mut sender, database).await?;
        assert_eq!(
            outcome(&mut sender, overflow).await?,
            ForwardOutcome::Overloaded
        );
        control(
            &reopened,
            RuntimeControl::SetTime {
                now: RegistryTime((started + std::time::Duration::from_secs(30)).into()),
            },
        )
        .await?;
        // Observe the owning database's actual alarm result before sending a
        // fresh request; SetTime schedules the alarm but does not await it.
        await_empty_pending(root.path()).await?;
        let replacement = post(&mut sender, database).await?;
        let fresh = holder_frame(&mut holder).await?;
        let reply = answer(&mut holder, fresh).await?;
        assert_eq!(
            outcome(&mut sender, replacement).await?,
            ForwardOutcome::Answered { reply }
        );
        close(sender).await?;
        close(holder).await?;
        Ok(())
    }
    .await;
    let stopped = reopened.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    proof
}

async fn await_empty_pending(root: &std::path::Path) -> ConformanceResult<()> {
    use sqlx_core::{
        connection::{ConnectOptions, Connection},
        query::query,
        row::Row,
    };
    let mut reader = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(format!("connection-{SENDER_SERVICE}.db")))
        .read_only(true)
        .connect()
        .await?;
    let proof = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let row =
                query::<sqlx_turso::Turso>("SELECT count(*) AS pending FROM pending_forwards")
                    .fetch_one(&mut reader)
                    .await?;
            if row.try_get::<i64, _>("pending")? == 0 {
                return Ok(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await?;
    reader.close().await?;
    proof
}
