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
use organization_stamps::{
    self as stamps, ADMIN_AUDIENCE, ISSUER, NOW_SECONDS, PUBLIC_ORIGIN, ProofKey, SignInKeys,
};
use serde_json::{Value, json};
use std::{path::PathBuf, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};

const OTHER: &str = "019f0000-0000-7000-8000-00000000b2c1";
const THIRD: &str = "019f0000-0000-7000-8000-00000000b2c2";
const LEGACY_MACHINE: &str = "550e8400-e29b-41d4-a716-44665544b2a8";

/// The stamp-profile refusals, each a correctly signed statement but for one field.
fn profile_negatives(organization: &str) -> ConformanceResult<Vec<(&'static str, String)>> {
    let with = |field: &str, value: Value| -> ConformanceResult<String> {
        let mut claims = stamps::claims(organization);
        stamps::set_claim(&mut claims, field, value)?;
        stamps::sign(&stamps::stamp_header(), &claims, 7)
    };
    Ok(vec![
        (
            "edited signature",
            stamps::tamper_signature(&stamps::stamp(organization)?)?,
        ),
        ("wrong purpose", with("purpose", json!("fleet-person"))?),
        (
            "wrong audience",
            with("aud", json!("https://other.invalid"))?,
        ),
        ("wrong issuer", with("iss", json!("https://other.invalid"))?),
        (
            "longer than 120 days",
            with(
                "exp",
                json!(NOW_SECONDS - 60 + stamps::LIFETIME_SECONDS + 1),
            )?,
        ),
        (
            "wrong type",
            stamps::sign(
                &json!({"typ":"at+jwt","alg":"EdDSA","kid":"stamp-a"}),
                &stamps::claims(organization),
                7,
            )?,
        ),
        (
            "unpublished key",
            stamps::sign(
                &json!({"typ":"fleet-organization-stamp+jwt","alg":"EdDSA","kid":"stamp-z"}),
                &stamps::claims(organization),
                9,
            )?,
        ),
    ])
}
const ADMIN_OWNER: &str = "019f0000-0000-7000-8000-00000000b2e1";
const ADMIN_SUBJECT: &str = "019f0000-0000-7000-8000-00000000b2e2";
const ADMIN_MEMBER: &str = "019f0000-0000-7000-8000-00000000b2e3";
const ADMIN_MEMBER_SUBJECT: &str = "019f0000-0000-7000-8000-00000000b2e4";
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
    /// binding carries no Access assertion (the private entry must not need
    /// one); "/public" observer reads carry the owner's.
    async fn post(&self, path: &str, body: &Value) -> ConformanceResult<(u16, String, Value)> {
        let mut request = self.http.post(format!("{}{path}", self.base)).json(body);
        if path.starts_with("/public/") {
            request = request.header("Cf-Access-Jwt-Assertion", &self.owner);
        }
        let response = request.send().await?;
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
    /// A person's admin call over the link with a real token and DPoP proof.
    async fn admin(
        &self,
        path: &str,
        intent: &Value,
        body: &Value,
        person: &AdminPerson,
        jti_serial: u16,
    ) -> ConformanceResult<(u16, Value)> {
        let token = person.token()?;
        let millis = NOW_SECONDS * 1000 - 1000;
        let jti = format!(
            "{:08x}-{:04x}-7000-8000-{:012x}",
            millis >> 16,
            millis & 0xffff,
            jti_serial
        );
        let proof = person.key.proof(&json!({
            "jti":jti,"htm":"POST","htu":format!("{PUBLIC_ORIGIN}{path}"),"iat":NOW_SECONDS - 1,
            "ath":stamps::sha256_url(token.as_bytes()),
            "intent":stamps::sha256_url(stamps::canonical(intent).as_bytes())
        }))?;
        let response = self
            .http
            .post(format!("{}/link{path}", self.base))
            .header("authorization", format!("DPoP {token}"))
            .header("dpop", proof)
            .json(body)
            .send()
            .await?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await?;
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)?
        };
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

struct AdminPerson {
    id: &'static str,
    subject: &'static str,
    organization: String,
    permissions: Vec<&'static str>,
    key: ProofKey,
}
impl AdminPerson {
    fn token(&self) -> ConformanceResult<String> {
        stamps::sign(
            &json!({"typ":"at+jwt","alg":"EdDSA","kid":"stamp-a"}),
            &json!({
                "iss":ISSUER,"aud":[ADMIN_AUDIENCE],"sub":self.id,
                "sid":"019f0000-0000-7000-8000-00000000b2f0",
                "iat":NOW_SECONDS - 10,"exp":NOW_SECONDS + 290,"cnf":{"jkt":self.key.thumbprint()?},
                "token_profile":"person-fleet","organization_id":self.organization,
                "authorization_subject_id":self.subject,"person_revision":1,"organization_revision":1,
                "permission_policy_digest":ADMIN_POLICY,"permissions":self.permissions,
                "second_factor_at":NOW_SECONDS - 60,"second_factor_method":"app",
                "session_expires_at_ms":(NOW_SECONDS + 86_400) * 1000,
                "authorization_snapshot_at_ms":NOW_SECONDS * 1000 - 10_000,
                "authorization_snapshot_valid_until_ms":NOW_SECONDS * 1000 + 200_000
            }),
            7,
        )
    }
}
const ADMIN_POLICY: &str = "2d31b5d72716e64e255d8858c231f593155289fb03ed354c4b762890667a3c22";

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
        // Today's data in the legacy store before it is stamped: a machine.
        let seeded = caller
            .post(
                "/public/admin/v1/machines/enroll",
                &json!({"kind":"enrollMachine","value":{
                    "label":"legacy machine","installation":LEGACY_MACHINE,
                    "peer":"019f0000-0000-7000-8000-00000000b2a8",
                    "access_client_id":collaboration_central_conformance::access_client_id("legacy-machine")?,
                    "credential_hash":stamps::digest("legacy-machine"),"expires_at":"2033-05-18T03:33:20Z"}}),
            )
            .await?;
        require(
            seeded.2.pointer("/kind") == Some(&json!("machineEnrolled")),
            "the legacy store holds today's data before provisioning",
            &seeded,
        )?;
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
        // The full stamp profile is checked on Cloudflare too, before any write.
        let mut negatives = Vec::new();
        for (case, statement) in profile_negatives(THIRD)? {
            negatives.push((case, caller.provision(THIRD, &statement).await?));
        }
        let third_rows = caller.records(THIRD).await?;
        require(
            negatives.iter().all(|(_, status)| *status == 422)
                && third_rows.get("statement") == Some(&Value::Null),
            "every profile negative is refused before a write",
            &(negatives, third_rows),
        )?;
        // Stamp and record commit together: a fault after the record write
        // leaves no stamp, and the retry commits both.
        caller
            .post(
                "/public/admin/v1/conformance/runtime",
                &json!({"address":{"kind":"fleet","value":{"organization":THIRD}},
                    "command":{"kind":"failNextOrganizationRecord"}}),
            )
            .await?;
        let faulted = caller.provision(THIRD, &stamps::stamp(THIRD)?).await?;
        let after_fault = caller.digest(THIRD).await?;
        let retried_third = caller.provision(THIRD, &stamps::stamp(THIRD)?).await?;
        let third_after = caller.records(THIRD).await?;
        require(
            faulted == 503
                && after_fault.0 == 404
                && retried_third == 200
                && summary(&third_after).len() == 1,
            "a faulted record leaves no stamp and the retry commits both",
            &(faulted, after_fault, retried_third, third_after),
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
        let public_admin = caller
            .post(
                "/public/organization-admin/v1/create-workspace",
                &json!({"organizationId":legacy}),
            )
            .await?;
        require(
            (public_admin.0, public_admin.1.as_str()) == (404, "collaboration-central-public-entry"),
            "public central has no admin route",
            &public_admin,
        )?;
        let unproved_admin = caller
            .post(
                "/link/organization-admin/v1/create-workspace",
                &json!({"organizationId":legacy,"operationId":"019f0000-0000-7000-8000-00000000b2d1",
                    "name":"research","description":""}),
            )
            .await?;
        require(
            unproved_admin.0 == 401,
            "a link admin call without a DPoP-bound person token is refused",
            &unproved_admin,
        )?;
        // Floors pushed over the link, then a person's admin journey through
        // central-link to the legacy Fleet object.
        let early_person = caller
            .post(
                "/link/internal/organization-authorization-revisions",
                &json!({"kind":"person","organizationId":legacy,"personId":ADMIN_OWNER,
                    "subjectId":ADMIN_SUBJECT,"revision":1,"state":"active"}),
            )
            .await?;
        let organization_floor = caller
            .post(
                "/link/internal/organization-authorization-revisions",
                &json!({"kind":"organization","organizationId":legacy,"revision":1,"state":"active",
                    "permissionPolicyDigest":ADMIN_POLICY}),
            )
            .await?;
        let mut floors = Vec::new();
        for (person, subject) in [(ADMIN_OWNER, ADMIN_SUBJECT), (ADMIN_MEMBER, ADMIN_MEMBER_SUBJECT)] {
            floors.push(
                caller
                    .post(
                        "/link/internal/organization-authorization-revisions",
                        &json!({"kind":"person","organizationId":legacy,"personId":person,
                            "subjectId":subject,"revision":1,"state":"active"}),
                    )
                    .await?
                    .0,
            );
        }
        require(
            (early_person.0, organization_floor.0, floors.clone()) == (412, 200, vec![200, 200]),
            "floors push over the binding, organization first",
            &(early_person, organization_floor, floors),
        )?;
        let owner = AdminPerson {
            id: ADMIN_OWNER,
            subject: ADMIN_SUBJECT,
            organization: legacy.to_owned(),
            permissions: vec!["organization:view", "workspace:create"],
            key: ProofKey::generate()?,
        };
        let member = AdminPerson {
            id: ADMIN_MEMBER,
            subject: ADMIN_MEMBER_SUBJECT,
            organization: legacy.to_owned(),
            permissions: vec!["organization:view"],
            key: ProofKey::generate()?,
        };
        let operation = "019f0000-0000-7000-8000-00000000b2e0";
        let intent = json!({"v":1,"operation":"CreateWorkspace","organization":legacy,"target":legacy,
            "operationId":operation,"params":{"name":"research","description":""}});
        let body = json!({"organizationId":legacy,"operationId":operation,"name":"research","description":""});
        let created = caller.admin("/organization-admin/v1/create-workspace", &intent, &body, &owner, 1).await?;
        let retried = caller.admin("/organization-admin/v1/create-workspace", &intent, &body, &owner, 2).await?;
        let reused = caller.admin("/organization-admin/v1/create-workspace", &intent, &body, &owner, 2).await?;
        let by_member = caller.admin("/organization-admin/v1/create-workspace", &intent, &body, &member, 3).await?;
        require(
            created.0 == 200
                && created.1.pointer("/result") == Some(&json!("applied"))
                && created.1.pointer("/workspace/workspaceId") == Some(&json!(operation))
                && retried.1.pointer("/alreadyExisted") == Some(&json!(true))
                && reused.1 == json!({"result":"refused","reason":"requestReused"})
                && by_member.1 == json!({"result":"refused","reason":"notPermitted"}),
            "a person creates one workspace through central-link; retry, replay and a member behave",
            &(created, retried, reused, by_member),
        )?;
        let lookup_intent = json!({"v":1,"operation":"WorkspaceLookup","organization":legacy,
            "target":operation,"operationId":null,"params":{}});
        let looked_up = caller
            .admin(
                "/organization-admin/v1/workspace-lookup",
                &lookup_intent,
                &json!({"organizationId":legacy,"workspaceId":operation}),
                &member,
                4,
            )
            .await?;
        require(
            looked_up.1.pointer("/workspace/name") == Some(&json!("research")),
            "a member reads the workspace",
            &looked_up,
        )?;
        let collision = caller
            .post(
                "/public/admin/v1/machines/enroll",
                &json!({"kind":"enrollMachine","value":{
                    "label":"collision","installation":"019f0000-0000-7000-8000-00000000e501",
                    "peer":"019f0000-0000-7000-8000-00000000b2e9",
                    "access_client_id":collaboration_central_conformance::access_client_id("collision")?,
                    "credential_hash":stamps::digest("collision"),"expires_at":"2033-05-18T03:33:20Z"}}),
            )
            .await?;
        require(
            collision.2 == json!({"kind":"refused","value":"invalidInput"}),
            "no machine is enrolled under the sign-in service's ServiceId",
            &collision,
        )?;
        let link_unknown = caller
            .post("/link/internal/organization-admin", &json!({}))
            .await?;
        require(
            link_unknown.0 == 404,
            "central-link refuses a path outside its six closed routes",
            &link_unknown,
        )?;
        // Damage OTHER's stored statement in place; the restart must refuse it.
        let tampered = caller
            .post(
                "/public/admin/v1/conformance/organization-statement-tamper",
                &json!({"organization":OTHER,"statement":stamps::tamper_signature(&other_statement)?}),
            )
            .await?;
        require(tampered.0 == 200, "the stored statement was overwritten", &tampered)?;
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
