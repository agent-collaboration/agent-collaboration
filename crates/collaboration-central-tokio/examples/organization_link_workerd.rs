//! B2b on Cloudflare: a caller Worker provisions through its CENTRAL binding to
//! the compiled central-link artifact, which reaches central's own Fleet
//! objects. Public central reads the same rows back, before and after an owned
//! workerd restart. The clock is the conformance build's fixed time.
#[path = "organization_link_workerd/link_client.rs"]
mod link_client;
#[path = "../tests/support/organization_stamps.rs"]
#[expect(
    dead_code,
    reason = "the Unix-socket client in the shared stamp fixtures is Tokio-only"
)]
mod organization_stamps;
#[path = "organization_link_workerd/record_checks.rs"]
mod record_checks;
#[path = "organization_link_workerd/refusal_cases.rs"]
mod refusal_cases;
#[path = "organization_link_workerd/stamp_time_cases.rs"]
mod stamp_time_cases;
#[path = "../tests/support/statement_refusals.rs"]
mod statement_refusals;
use collaboration_central_conformance::{
    ConformanceResult, DispatchCallSelector, DispatchKey, FIXTURE_KID, JwksFixtureServer,
    RuntimeControl, RuntimeControlRequest, fixture_policy, legacy_organization, sign_assertion,
};
use link_client::{AdminPerson, Caller, LinkHarness};
use organization_stamps::{
    self as stamps, B2_PUBLIC_PATHS_NOT_SERVED, NOW_SECONDS, ProofKey, SignInKeys,
};
use record_checks::{created_records, observation_whole, origins_closed, require, summary};
use serde_json::{Value, json};

const OTHER: &str = "019f0000-0000-7000-8000-00000000b2c1";
const THIRD: &str = "019f0000-0000-7000-8000-00000000b2c2";
/// Never provisioned: a non-legacy organization with no stamp.
const UNSTAMPED: &str = "019f0000-0000-7000-8000-00000000b2c3";
/// Provisioned only by the barrier-forced races.
const RACE: &str = "019f0000-0000-7000-8000-00000000b2c4";
/// Provisioned only by the byte-identical first-provision race.
const RACE_SAME: &str = "019f0000-0000-7000-8000-00000000b2c5";
/// Stamped under the second published key, which is retired before the restart.
const KEYED: &str = "019f0000-0000-7000-8000-00000000b2c6";
/// Never provisioned; offered a statement under the retired key after the restart.
const AFTER_RETIREMENT: &str = "019f0000-0000-7000-8000-00000000b2c7";
const LEGACY_MACHINE: &str = "550e8400-e29b-41d4-a716-44665544b2a8";

const ADMIN_OWNER: &str = "019f0000-0000-7000-8000-00000000b2e1";
const ADMIN_SUBJECT: &str = "019f0000-0000-7000-8000-00000000b2e2";
const ADMIN_MEMBER: &str = "019f0000-0000-7000-8000-00000000b2e3";
const ADMIN_MEMBER_SUBJECT: &str = "019f0000-0000-7000-8000-00000000b2e4";
const FIXED_MILLIS: i64 = NOW_SECONDS * 1000;

const ADMIN_POLICY: &str = "2d31b5d72716e64e255d8858c231f593155289fb03ed354c4b762890667a3c22";

const LOST_OPERATION: &str = "019f0000-0000-7000-8000-00000000b2f1";
fn lost_intent(organization: &str) -> Value {
    json!({"v":1,"operation":"CreateWorkspace","organization":organization,"target":organization,
        "operationId":LOST_OPERATION,"params":{"name":"lost answer","description":""}})
}
fn lost_body(organization: &str) -> Value {
    json!({"organizationId":organization,"operationId":LOST_OPERATION,"name":"lost answer","description":""})
}
/// One conformance control at `organization`'s Fleet object, through public
/// central's owner route.
async fn control(
    caller: &Caller,
    organization: &str,
    command: RuntimeControl,
) -> ConformanceResult<()> {
    let request = serde_json::to_value(RuntimeControlRequest {
        address: collaboration_central_core::ObjectAddress::Fleet {
            organization: organization.to_owned().try_into()?,
        },
        command,
    })?;
    let answer = caller
        .post("/public/admin/v1/conformance/runtime", &request)
        .await?;
    if answer.0 != 200 {
        return Err(format!("runtime control refused: {answer:?}").into());
    }
    Ok(())
}

/// Hold `held`'s provisioning after it verifies and before its transaction,
/// commit `racing`, then release `held`; both answers, held first.
async fn race(
    caller: &Caller,
    organization: &str,
    held: String,
    racing: String,
) -> ConformanceResult<(u16, u16)> {
    let key = DispatchKey::new(
        collaboration_central_core::ObjectAddress::Fleet {
            organization: organization.to_owned().try_into()?,
        },
        DispatchCallSelector::OrganizationProvisioning,
    );
    control(
        caller,
        organization,
        RuntimeControl::HoldNextDispatch { key: key.clone() },
    )
    .await?;
    let held_caller = caller.clone();
    let held_organization = organization.to_owned();
    let held_call = tokio::spawn(async move {
        held_caller
            .provision(&held_organization, &held)
            .await
            .map_err(|error| error.to_string())
    });
    // Returns once the held call has verified and stopped before applying.
    control(
        caller,
        organization,
        RuntimeControl::AwaitHeldDispatch { key: key.clone() },
    )
    .await?;
    let racing_answer = caller.provision(organization, &racing).await?;
    control(caller, organization, RuntimeControl::ResumeDispatch { key }).await?;
    let held_answer = held_call.await??;
    Ok((held_answer, racing_answer))
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
        // B2's own routes refuse on any unstamped store and reveal nothing,
        // legacy or not. The Access-era enrollment above is the transition's
        // positive control; Program Design step 2 removes those routes.
        let mut unstamped_b2 = Vec::new();
        for (index, organization) in [legacy, UNSTAMPED].into_iter().enumerate() {
            let person = AdminPerson {
                organization_revision: 1,
                id: ADMIN_OWNER,
                subject: ADMIN_SUBJECT,
                organization: organization.to_owned(),
                permissions: vec!["organization:view", "workspace:create"],
                key: ProofKey::generate()?,
            };
            let workspace = "019f0000-0000-7000-8000-00000000b2d9";
            let serial = u16::try_from(100 + 2 * index)?;
            unstamped_b2.push(
                caller
                    .admin(
                        "/organization-admin/v1/workspace-lookup",
                        &json!({"v":1,"operation":"WorkspaceLookup","organization":organization,
                            "target":workspace,"operationId":null,"params":{}}),
                        &json!({"organizationId":organization,"workspaceId":workspace}),
                        &person,
                        serial,
                    )
                    .await?,
            );
            unstamped_b2.push(
                caller
                    .admin(
                        "/organization-admin/v1/create-workspace",
                        &json!({"v":1,"operation":"CreateWorkspace","organization":organization,
                            "target":organization,"operationId":workspace,
                            "params":{"name":"research","description":""}}),
                        &json!({"organizationId":organization,"operationId":workspace,
                            "name":"research","description":""}),
                        &person,
                        serial + 1,
                    )
                    .await?,
            );
            let push = caller
                .post(
                    "/link/internal/organization-authorization-revisions",
                    &json!({"kind":"organization","organizationId":organization,"revision":1,
                        "state":"active","permissionPolicyDigest":ADMIN_POLICY}),
                )
                .await?;
            unstamped_b2.push((push.0, push.2));
        }
        require(
            unstamped_b2.iter().all(|answer| answer == &(503, Value::Null)),
            "B2 link routes on an unstamped legacy or non-legacy store refuse and reveal nothing",
            &unstamped_b2,
        )?;
        let mut b2_public = Vec::new();
        for (method, path) in B2_PUBLIC_PATHS_NOT_SERVED {
            let unserved = caller.send(method, "/public/no-such-route", &json!({})).await?;
            let answer = caller.send(method, &format!("/public{path}"), &json!({})).await?;
            b2_public.push((*method, *path, (answer.0, answer.2), (unserved.0, unserved.2)));
        }
        require(
            b2_public.len() == 2
                && b2_public.iter().all(|(_, _, answer, unserved)| answer == unserved),
            "each B2 public path answers exactly like an unserved path with its own method",
            &b2_public,
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
                && summary(&other_rows).len() == 1
                && origins_closed(&legacy_rows)
                && origins_closed(&other_rows),
            "a non-legacy organization's object holds only its own stamp",
            &other_rows,
        )?;
        // The full stamp profile is checked on Cloudflare too, before any write.
        let mut negatives = Vec::new();
        for refusal in statement_refusals::profile_refusals(THIRD)? {
            negatives.push((refusal.case, caller.provision(THIRD, &refusal.statement).await?));
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
                && summary(&third_after).len() == 1
                && origins_closed(&third_after),
            "a faulted record leaves no stamp and the retry commits both",
            &(faulted, after_fault, retried_third, third_after),
        )?;
        // Barrier-forced races: a held provisioning verified before another
        // committed must decide on what its own transaction reads.
        let first_held = stamps::stamp(RACE)?;
        let first_racing = stamps::first_statement(RACE, 1)?;
        let first_race = race(&caller, RACE, first_held, first_racing.clone()).await?;
        let after_first = caller.records(RACE).await?;
        require(
            first_race == (200, 200)
                && after_first.get("statement") == Some(&json!(first_racing))
                && summary(&after_first).len() == 1
                && origins_closed(&after_first),
            "two different first provisions at once: both Provisioned, the first committed is stored, one setup record",
            &(first_race, after_first),
        )?;
        let same = stamps::stamp(RACE_SAME)?;
        let same_race = race(&caller, RACE_SAME, same.clone(), same.clone()).await?;
        let after_same = caller.records(RACE_SAME).await?;
        require(
            same_race == (200, 200)
                && after_same.get("statement") == Some(&json!(same))
                && summary(&after_same).len() == 1,
            "two byte-identical first provisions at once: both Provisioned, one stamp, one record",
            &(same_race, after_same),
        )?;
        let keyed = caller
            .provision(KEYED, &stamps::stamp_under_second_key(KEYED)?)
            .await?;
        require(
            keyed == 200 && caller.digest(KEYED).await?.0 == 200,
            "a stamp under the second published key is accepted while that key is published",
            &keyed,
        )?;
        // Statement time and the warm key cache, each on its own Fleet object.
        stamp_time_cases::run(&caller, &keys).await?;
        let renewal_held = stamps::renewal(RACE, &first_racing, 10)?;
        let renewal_racing = stamps::renewal(RACE, &first_racing, 11)?;
        let renewal_race = race(&caller, RACE, renewal_held, renewal_racing.clone()).await?;
        let after_renewals = caller.records(RACE).await?;
        require(
            renewal_race == (409, 200)
                && after_renewals.get("statement") == Some(&json!(renewal_racing))
                && summary(&after_renewals).len() == 2,
            "two renewals of one stamp at once: the racing one wins, the held one is refused",
            &(renewal_race, after_renewals),
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
        let mut link_on_public = Vec::new();
        for path in [
            "/internal/organization-provisioning",
            "/internal/organization-statement-digest",
            "/internal/organization-authorization-revisions",
            "/organization-admin/v1/workspace-lookup",
            "/organization-admin/v1/change-records",
        ] {
            let answer = caller
                .post(&format!("/public{path}"), &json!({"organizationId":legacy}))
                .await?;
            link_on_public.push((answer.0, answer.1));
        }
        require(
            link_on_public
                .iter()
                .all(|(status, entry)| *status == 404 && entry == "collaboration-central-public-entry"),
            "public central serves none of the private link routes, metadata included",
            &link_on_public,
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
                organization_revision: 1,
            id: ADMIN_OWNER,
            subject: ADMIN_SUBJECT,
            organization: legacy.to_owned(),
            permissions: vec!["organization:view", "workspace:create"],
            key: ProofKey::generate()?,
        };
        let member = AdminPerson {
                organization_revision: 1,
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
        // A reply lost after central commits: the workspace and its record are
        // already stored; the same operation after the restart finds them.
        caller
            .post(
                "/public/admin/v1/conformance/runtime",
                &json!({"address":{"kind":"fleet","value":{"organization":legacy}},
                    "command":{"kind":"dropNextOrganizationAnswer"}}),
            )
            .await?;
        let lost = caller
            .admin(
                "/organization-admin/v1/create-workspace",
                &lost_intent(legacy),
                &lost_body(legacy),
                &owner,
                20,
            )
            .await?;
        let lost_rows = caller.records(legacy).await?;
        require(
            lost == (503, Value::Null)
                && created_records(&lost_rows, LOST_OPERATION) == 1,
            "a lost answer comes after the commit",
            &(lost, lost_rows),
        )?;
        // A floor apply aborted after its write: no ACK, and the store still
        // holds the prior floor before any retry (an idempotent retry would
        // hide a write that committed behind the 503).
        caller
            .post(
                "/public/admin/v1/conformance/runtime",
                &json!({"address":{"kind":"fleet","value":{"organization":legacy}},
                    "command":{"kind":"failNextOrganizationRecord"}}),
            )
            .await?;
        let raised_floor = json!({"kind":"organization","organizationId":legacy,"revision":2,
            "state":"active","permissionPolicyDigest":ADMIN_POLICY});
        let faulted_floor = caller
            .post("/link/internal/organization-authorization-revisions", &raised_floor)
            .await?;
        let floor_after_fault = caller.records(legacy).await?;
        let retried_floor = caller
            .post("/link/internal/organization-authorization-revisions", &raised_floor)
            .await?;
        require(
            faulted_floor.0 == 503
                && floor_after_fault.pointer("/organizationFloor/revision") == Some(&json!(1))
                && retried_floor.2 == json!({"acknowledgedRevision":2}),
            "an aborted floor apply leaves the prior floor and never acknowledges",
            &(faulted_floor, floor_after_fault, retried_floor),
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
        let final_rows = caller.records(legacy).await?;
        require(
            origins_closed(&final_rows)
                && summary(&final_rows)
                    .iter()
                    .any(|(_, kind, _)| kind == "workspaceCreated"),
            "setup records name only the sign-in service and the admin record its person, read back from central",
            &final_rows,
        )?;
        require(
            observation_whole(&final_rows),
            "the observer reads the whole organization from its Fleet object: stamp facts, person floors, used requests, workspaces and decisions",
            &final_rows,
        )?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(final_rows)
    }
    .await;
    harness.stop().await?;
    let before_restart = first?;
    // The sign-in service retires its second key once every central restarted.
    keys.retire("stamp-b")?;
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
        // The lost answer's operation, retried after the restart by the same
        // owner (whose token now carries the raised organization floor).
        let owner_again = AdminPerson {
            organization_revision: 2,
            id: ADMIN_OWNER,
            subject: ADMIN_SUBJECT,
            organization: legacy.to_owned(),
            permissions: vec!["organization:view", "workspace:create"],
            key: ProofKey::generate()?,
        };
        let recovered = caller
            .admin(
                "/organization-admin/v1/create-workspace",
                &lost_intent(legacy),
                &lost_body(legacy),
                &owner_again,
                21,
            )
            .await?;
        let recovered_rows = caller.records(legacy).await?;
        require(
            recovered.0 == 200
                && recovered.1.pointer("/result") == Some(&json!("applied"))
                && recovered.1.pointer("/alreadyExisted") == Some(&json!(true))
                && recovered.1.pointer("/workspace/workspaceId") == Some(&json!(LOST_OPERATION))
                && created_records(&recovered_rows, LOST_OPERATION) == 1,
            "the same operation finds its workspace after the restart: one workspace, one record",
            &(recovered, recovered_rows),
        )?;
        // A stamp under the retired key is refused after the cold start; a new
        // statement under it is refused before any write.
        let retired_digest = caller.digest(KEYED).await?;
        let retired_setup = caller.provision(KEYED, &stamps::stamp(KEYED)?).await?;
        let retired_new = caller
            .provision(
                AFTER_RETIREMENT,
                &stamps::stamp_under_second_key(AFTER_RETIREMENT)?,
            )
            .await?;
        let retired_rows = caller.records(AFTER_RETIREMENT).await?;
        require(
            retired_digest.0 == 503
                && retired_setup == 409
                && retired_new == 422
                && retired_rows.get("statement") == Some(&Value::Null),
            "a retired key's stamp is refused after the cold start and a new statement under it writes nothing",
            &(retired_digest, retired_setup, retired_new, retired_rows),
        )?;
        // OTHER's stored statement was damaged in place before the restart:
        // every call naming OTHER is refused, the damage is still what is
        // stored, and the valid legacy organization is the control above.
        let damaged_digest = caller.digest(OTHER).await?;
        let damaged_setup = caller.provision(OTHER, &other_statement).await?;
        let damaged_push = caller
            .post(
                "/link/internal/organization-authorization-revisions",
                &json!({"kind":"organization","organizationId":OTHER,"revision":1,
                    "state":"active","permissionPolicyDigest":ADMIN_POLICY}),
            )
            .await?;
        let damaged_rows = caller.records(OTHER).await?;
        require(
            damaged_digest.0 == 503
                && damaged_setup == 409
                && damaged_push.0 == 503
                && damaged_rows.get("statement")
                    == Some(&json!(stamps::tamper_signature(&other_statement)?))
                && damaged_rows.get("organizationFloor") == Some(&Value::Null),
            "a damaged stored stamp refuses every call naming its organization after the restart and stays as stored",
            &(damaged_digest, damaged_setup, damaged_push, damaged_rows),
        )?;
        let sent = refusal_cases::send(&caller).await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(sent)
    }
    .await;
    let restarted_lines = restarted.stop().await?;
    keys.shutdown().await?;
    access.shutdown().await?;
    refusal_cases::judge(&second?, &restarted_lines)?;
    println!("LINK JOURNEY passed");
    Ok(())
}
