//! S8 and seam-control failures through the owner's runtime-control route.
#![cfg(feature = "conformance")]
use crate::configuration;
use collaboration_central_conformance::{
    ConformanceResult, JwksFixtureServer, RuntimeControl, RuntimeControlRequest, fixture_policy,
    owner_assertion,
};
use collaboration_central_core::ObjectAddress;
use collaboration_central_tokio::{TokioServer, TokioSettings};
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

fn settings(root: &Path, jwks: &str) -> ConformanceResult<TokioSettings> {
    Ok(TokioSettings {
        root: root.into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.into(),
        registry: configuration::registry_config()?,
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
}

/// Post one runtime control as the owner: the status and the JSON body.
async fn control(
    base: &str,
    address: ObjectAddress,
    command: RuntimeControl,
) -> ConformanceResult<(StatusCode, Value)> {
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?
        .post(format!("{base}/admin/v1/conformance/runtime"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&RuntimeControlRequest { address, command })
        .send()
        .await?;
    Ok((response.status(), response.json().await?))
}

#[tokio::test]
async fn snapshot_and_seam_failures_answer_their_contract_bodies_over_http() -> ConformanceResult<()>
{
    // Arrange
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start(settings(root.path(), &jwks.url)?).await?;
    let proof = async {
        let copy = ObjectAddress::Copy(fleet_protocol::DatabaseId::new(
            durable_steps::Uuid::now_v7(),
        )?);

        // Act and assert: an untouched Copy has no open store. The 409 body is
        // the failure's own JSON, exactly as seam contract v3.4/v3.6 write it.
        let (status, body) = control(server.base_url(), copy, RuntimeControl::Snapshot).await?;
        if (status, &body) != (StatusCode::CONFLICT, &json!("notOpen")) {
            return Err(format!("notOpen answered {status} {body}").into());
        }
        // An existing seam failure answers its literal name too.
        let (status, body) = control(
            server.base_url(),
            ObjectAddress::Fleet,
            RuntimeControl::AwaitBarrier {
                barrier_id: durable_steps::Uuid::now_v7(),
            },
        )
        .await?;
        if (status, &body) != (StatusCode::CONFLICT, &json!("unknownBarrier")) {
            return Err(format!("unknownBarrier answered {status} {body}").into());
        }
        // A fresh server has not opened its Fleet: snapshot refuses until an
        // ordinary owner read opens it.
        let (status, body) = control(
            server.base_url(),
            ObjectAddress::Fleet,
            RuntimeControl::Snapshot,
        )
        .await?;
        if (status, &body) != (StatusCode::CONFLICT, &json!("notOpen")) {
            return Err(format!("unopened Fleet answered {status} {body}").into());
        }
        let opened = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()?
            .get(format!("{}/admin/v1/copies?limit=1", server.base_url()))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .send()
            .await?
            .status();
        if opened != StatusCode::OK {
            return Err(format!("the owner read that opens the Fleet answered {opened}").into());
        }
        // The opened Fleet answers a snapshot with every v3.6 field.
        let (status, body) = control(
            server.base_url(),
            ObjectAddress::Fleet,
            RuntimeControl::Snapshot,
        )
        .await?;
        let fields = body
            .pointer("/kind")
            .zip(body.pointer("/value"))
            .and_then(|(kind, value)| (kind == "snapshot").then_some(value))
            .and_then(Value::as_object)
            .map(|value| {
                let mut keys: Vec<&str> = value.keys().map(String::as_str).collect();
                keys.sort_unstable();
                keys
            });
        if status != StatusCode::OK
            || fields
                != Some(vec![
                    "attempts",
                    "feed",
                    "inventory",
                    "noticeProgress",
                    "owners",
                    "sessions",
                    "windows",
                ])
        {
            return Err(format!("Fleet snapshot answered {status} {body}").into());
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    server.shutdown().await?;
    proof
}
