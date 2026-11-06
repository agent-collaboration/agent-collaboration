//! Physical Fleet scope and new-shape DO metadata survive an actual workerd restart.
//! This proves routing/storage continuity, not SQL-gated organization provisioning.
#[path = "../../collaboration-central-conformance/examples/support/workerd_event_process.rs"]
mod workerd_event_process;
use collaboration_central_conformance::{
    ClockObservation, ConformanceResult, FIXTURE_KID, JwksFixtureServer, RuntimeControl,
    RuntimeControlRequest, RuntimeFaultObservation, fixture_policy, legacy_organization,
    sign_assertion,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use workerd_event_process::{EventWorkerdProcess, WorkerdSettings};
const ACCESS_TIME: i64 = 1_900_000_000_000;
struct ControlClient {
    http: reqwest::Client,
    url: String,
    assertion: String,
}
impl ControlClient {
    async fn control(
        &self,
        address: ObjectAddress,
        command: RuntimeControl,
    ) -> ConformanceResult<RuntimeFaultObservation> {
        Ok(self
            .http
            .post(&self.url)
            .header("Cf-Access-Jwt-Assertion", &self.assertion)
            .json(&RuntimeControlRequest { address, command })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
    async fn require_clock(
        &self,
        address: ObjectAddress,
        expected: RegistryTime,
    ) -> ConformanceResult<()> {
        if !matches!(self.control(address, RuntimeControl::Observe).await?.clock, ClockObservation::Pinned { now } if now == expected)
        {
            return Err("Fleet selected a different object's persisted marker".into());
        }
        Ok(())
    }
}
#[tokio::main]
async fn main() -> ConformanceResult<()> {
    let mut args = std::env::args().skip(1);
    let node = args.next().ok_or("Node executable required")?.into();
    let miniflare = args
        .next()
        .ok_or("pinned Miniflare entrypoint required")?
        .into();
    let persistence = args
        .next()
        .ok_or("fresh isolated persistence directory required")?
        .into();
    let port = args.next().ok_or("loopback port required")?.parse()?;
    if args.next().is_some() {
        return Err("unexpected arguments".into());
    }
    drop(std::net::TcpListener::bind(("127.0.0.1", port))?);
    let jwks = JwksFixtureServer::start().await?;
    let settings = WorkerdSettings {
        node,
        miniflare,
        persistence,
        port,
        jwks_url: jwks.url.clone(),
        legacy_organization: legacy_organization(),
        fixed_access_time: Some(ACCESS_TIME),
        relay_peer: "019f0000-0000-7000-8000-00000000ad04"
            .to_owned()
            .try_into()?,
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    };
    let policy = fixture_policy();
    let assertion = sign_assertion(
        &serde_json::json!({"iss":policy.issuer,"aud":[policy.admin_audience],"email":policy.owner_identity,"sub":"fixture-owner","type":"app","nbf":ACCESS_TIME / 1000 - 1,"exp":ACCESS_TIME / 1000 + 3600}),
        FIXTURE_KID,
    )?;
    let client = ControlClient {
        http: reqwest::Client::new(),
        url: format!("http://127.0.0.1:{port}/admin/v1/conformance/runtime"),
        assertion,
    };
    let mut cases = Vec::new();
    for (organization, offset) in [
        (legacy_organization(), 100),
        (
            "019f0000-0000-7000-8000-00000000ad02"
                .to_owned()
                .try_into()?,
            200,
        ),
        (
            "019f0000-0000-7000-8000-00000000ad03"
                .to_owned()
                .try_into()?,
            300,
        ),
    ] {
        cases.push((
            ObjectAddress::Fleet { organization },
            RegistryTime(
                chrono::DateTime::from_timestamp_millis(ACCESS_TIME + offset)
                    .ok_or("fixed time fixture")?,
            ),
        ));
    }
    let server = EventWorkerdProcess::start(&settings).await?;
    let prepare = async {
        for (address, now) in &cases {
            client
                .control(address.clone(), RuntimeControl::SetTime { now: *now })
                .await?;
        }
        for (address, now) in &cases {
            client.require_clock(address.clone(), *now).await?;
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    server.stop().await?;
    let proof = async {
        prepare?;
        let restarted = EventWorkerdProcess::start(&settings).await?;
        let check = async {
            for (address, now) in &cases {
                client.require_clock(address.clone(), *now).await?;
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
        restarted.stop().await?;
        check
    }
    .await;
    jwks.shutdown().await?;
    proof?;
    println!(
        "FLEET_LOCATION_WORKERD 3 isolated Fleet metadata markers preserved through owned process restart; new address shape only"
    );
    Ok(())
}
