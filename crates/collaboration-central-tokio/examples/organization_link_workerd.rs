//! B2b on Cloudflare: a caller Worker provisions through its CENTRAL binding to
//! the compiled central-link artifact, which reaches central's own Fleet
//! objects. Public central reads the same rows back, before and after an owned
//! workerd restart. The clock is the conformance build's fixed time.
#[path = "../tests/support/organization_stamps.rs"]
#[expect(
    dead_code,
    reason = "the Unix-socket client in the shared stamp fixtures is Tokio-only"
)]
mod organization_stamps;
use collaboration_central_conformance::{
    ConformanceResult, FIXTURE_KID, JwksFixtureServer, fixture_policy, legacy_organization,
    sign_assertion,
};
use organization_stamps::{self as stamps, NOW_SECONDS, SignInKeys};
use serde_json::{Value, json};
use std::{path::PathBuf, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};

const OTHER: &str = "019f0000-0000-7000-8000-00000000b2c1";
const FIXED_MILLIS: i64 = NOW_SECONDS * 1000;

struct LinkHarness {
    child: Child,
    output: tokio::task::JoinHandle<ConformanceResult<()>>,
}
impl LinkHarness {
    async fn start(arguments: &[String]) -> ConformanceResult<Self> {
        let mut child = Command::new(arguments.first().ok_or("node path")?)
            .current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .arg("scripts/tooling/run-central-link-workerd.ts")
            .args(arguments.get(1..).ok_or("harness arguments")?)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;
        let mut lines = BufReader::new(child.stdout.take().ok_or("harness stdout")?).lines();
        // Readiness is the harness's own ready line; no polling or sleep.
        loop {
            let line = lines
                .next_line()
                .await?
                .ok_or("harness ended before readiness")?;
            println!("{line}");
            if line.starts_with("Ready on ")
                && line.ends_with("(real workerd, central-link binding)")
            {
                break;
            }
        }
        let output = tokio::spawn(async move {
            while let Some(line) = lines.next_line().await? {
                println!("{line}");
            }
            Ok(())
        });
        Ok(Self { child, output })
    }
    async fn stop(mut self) -> ConformanceResult<()> {
        let pid = self.child.id().ok_or("harness already exited")?;
        let signal = Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status()
            .await?;
        if !signal.success() {
            return Err("could not signal the owned harness".into());
        }
        let status = self.child.wait().await?;
        self.output.await??;
        if !matches!(status.code(), Some(0 | 130)) {
            return Err(format!("owned harness exited: {status}").into());
        }
        println!("LINK HARNESS reaped pid={pid} exit={:?}", status.code());
        Ok(())
    }
}

struct Caller {
    http: reqwest::Client,
    base: String,
    owner: String,
}
impl Caller {
    /// One request through the fixture caller: "/link" over the CENTRAL
    /// binding, "/public" to public central.
    async fn post(&self, path: &str, body: &Value) -> ConformanceResult<(u16, String, Value)> {
        let response = self
            .http
            .post(format!("{}{path}", self.base))
            .header("Cf-Access-Jwt-Assertion", &self.owner)
            .json(body)
            .send()
            .await?;
        let status = response.status().as_u16();
        let entry = response
            .headers()
            .get("x-collaboration-central-entry")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("none")
            .to_owned();
        let bytes = response.bytes().await?;
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into()))
        };
        Ok((status, entry, body))
    }
    async fn provision(&self, organization: &str, statement: &str) -> ConformanceResult<u16> {
        Ok(self
            .post(
                "/link/internal/organization-provisioning",
                &json!({"organizationId":organization,"organizationStatement":statement}),
            )
            .await?
            .0)
    }
    async fn digest(&self, organization: &str) -> ConformanceResult<(u16, Value)> {
        let (status, _, body) = self
            .post(
                "/link/internal/organization-statement-digest",
                &json!({"organizationId":organization}),
            )
            .await?;
        Ok((status, body))
    }
    /// Read through public central's own Fleet routing, not the link.
    async fn records(&self, organization: &str) -> ConformanceResult<Value> {
        let (status, _, body) = self
            .post(
                "/public/admin/v1/conformance/organization-records",
                &json!({"organization":organization}),
            )
            .await?;
        if status != 200 {
            return Err(format!("organization records read failed: {status} {body}").into());
        }
        Ok(body)
    }
}

fn require(condition: bool, case: &str, observed: &impl std::fmt::Debug) -> ConformanceResult<()> {
    if condition {
        println!("LINK CASE passed: {case}");
        Ok(())
    } else {
        Err(format!("LINK CASE failed: {case}: observed {observed:?}").into())
    }
}

fn summary(records: &Value) -> Vec<(String, String, String)> {
    records
        .get("records")
        .and_then(Value::as_array)
        .map(|records| {
            records
                .iter()
                .map(|record| {
                    let field = |name: &str| {
                        record
                            .get(name)
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    };
                    (field("requestId"), field("kind"), field("targetId"))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::main]
async fn main() -> ConformanceResult<()> {
    let mut args = std::env::args().skip(1);
    let node = args.next().ok_or("Node executable required")?;
    let miniflare = args.next().ok_or("pinned Miniflare entrypoint required")?;
    let persistence = args
        .next()
        .ok_or("fresh isolated persistence directory required")?;
    let port: u16 = args.next().ok_or("loopback port required")?.parse()?;
    if args.next().is_some() {
        return Err("unexpected arguments".into());
    }
    drop(std::net::TcpListener::bind(("127.0.0.1", port))?);
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let legacy = legacy_organization();
    let legacy = legacy.as_str();
    let harness_arguments = vec![
        node,
        miniflare,
        persistence,
        port.to_string(),
        access.url.clone(),
        keys.url.clone(),
        legacy.to_owned(),
        FIXED_MILLIS.to_string(),
    ];
    let policy = fixture_policy();
    let caller = Caller {
        http: reqwest::Client::builder().no_proxy().build()?,
        base: format!("http://127.0.0.1:{port}"),
        owner: sign_assertion(
            &json!({"iss":policy.issuer,"aud":[policy.admin_audience],"email":policy.owner_identity,
                "sub":"fixture-owner","type":"app","nbf":NOW_SECONDS - 1,"exp":NOW_SECONDS + 3600}),
            FIXTURE_KID,
        )?,
    };
    let legacy_statement = stamps::stamp(legacy)?;
    let other_statement = stamps::stamp(OTHER)?;
    let renewal = stamps::renewal(legacy, &legacy_statement, 10)?;
    let late = stamps::renewal(legacy, &legacy_statement, 11)?;

    let harness = LinkHarness::start(&harness_arguments).await?;
    let first = async {
        let unstamped = caller.digest(legacy).await?;
        require(
            unstamped.0 == 404,
            "an unstamped Fleet has no digest",
            &unstamped,
        )?;
        let answers = vec![
            caller.provision(legacy, &legacy_statement).await?,
            caller.provision(legacy, &legacy_statement).await?,
            caller.provision(OTHER, &other_statement).await?,
            caller.provision(legacy, &other_statement).await?,
            caller
                .provision(legacy, &stamps::tamper_signature(&legacy_statement)?)
                .await?,
        ];
        require(
            answers == vec![200, 200, 200, 403, 422],
            "provision, repeat, other org, foreign statement, forged signature",
            &answers,
        )?;
        let legacy_rows = caller.records(legacy).await?;
        require(
            legacy_rows.get("statement") == Some(&json!(legacy_statement))
                && summary(&legacy_rows)
                    == vec![(
                        legacy.to_owned(),
                        "organizationProvisioned".to_owned(),
                        legacy.to_owned(),
                    )],
            "the legacy fleet object central reads holds the link's one stamp and record",
            &legacy_rows,
        )?;
        let other_rows = caller.records(OTHER).await?;
        require(
            other_rows.get("statement") == Some(&json!(other_statement))
                && summary(&other_rows).len() == 1,
            "a non-legacy organization's object holds only its own stamp",
            &other_rows,
        )?;
        let renewed = vec![
            caller.provision(legacy, &renewal).await?,
            caller.provision(legacy, &late).await?,
            caller.provision(legacy, &legacy_statement).await?,
        ];
        require(
            renewed == vec![200, 409, 200],
            "renewal, late stale, setup retry",
            &renewed,
        )?;
        let digest = caller.digest(legacy).await?;
        require(
            digest
                == (
                    200,
                    json!({"digest":stamps::digest(&renewal),
                        "expiresAt":NOW_SECONDS - 50 + stamps::LIFETIME_SECONDS,
                        "signingKeyId":"stamp-a"}),
                ),
            "the digest read reports the renewed stamp",
            &digest,
        )?;
        let public_internal = caller
            .post(
                "/public/internal/organization-provisioning",
                &json!({"organizationId":legacy,"organizationStatement":legacy_statement}),
            )
            .await?;
        require(
            (public_internal.0, public_internal.1.as_str())
                == (404, "collaboration-central-public-entry"),
            "public central has no private route",
            &public_internal,
        )?;
        let link_public = caller
            .post("/link/admin/v1/machines/enroll", &json!({}))
            .await?;
        require(
            (link_public.0, link_public.1.as_str()) == (404, "collaboration-central-link-entry"),
            "central-link serves no public route",
            &link_public,
        )?;
        let link_unknown = caller
            .post("/link/internal/organization-admin", &json!({}))
            .await?;
        require(
            link_unknown.0 == 404,
            "central-link exposes only its two contracts",
            &link_unknown,
        )?;
        caller.records(legacy).await
    }
    .await;
    harness.stop().await?;
    let before_restart = first?;
    let restarted = LinkHarness::start(&harness_arguments).await?;
    let second = async {
        let after_restart = caller.records(legacy).await?;
        require(
            after_restart == before_restart,
            "the stamp and records survive the owned workerd restart",
            &after_restart,
        )?;
        let digest = caller.digest(legacy).await?;
        require(
            digest.0 == 200,
            "the restarted Fleet still trusts its stamp",
            &digest,
        )?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    restarted.stop().await?;
    keys.shutdown().await?;
    access.shutdown().await?;
    second?;
    println!("LINK JOURNEY passed");
    Ok(())
}
