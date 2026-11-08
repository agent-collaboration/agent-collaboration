//! On workerd: each refused organization statement leaves exactly one Worker
//! log line naming its closed kind and the Fleet's organization, no line
//! carries statement material, and the conformance observer refuses a valid
//! assertion from a person who is not the owner.
use super::link_client::Caller;
use super::organization_stamps::{self as stamps, NOW_SECONDS};
use super::record_checks::require;
use super::statement_refusals::{StatementRefusal, further_refusals, profile_refusals};
use collaboration_central_conformance::{
    ConformanceResult, FIXTURE_KID, fixture_policy, sign_assertion,
};
use serde_json::json;

/// Names the statement used by the another-organization case.
const OTHER_STATEMENT: &str = "019f0000-0000-7000-8000-00000000b3af";

/// A refusal sent to its own organization, and the answer it drew.
pub(crate) struct SentRefusal {
    organization: String,
    refusal: StatementRefusal,
    status: u16,
}

fn case_organization(index: usize) -> String {
    format!("019f0000-0000-7000-8000-00000000b3{:02x}", 0xa0 + index)
}

/// Send every refusal case, each to a never-named organization, and check the
/// non-owner observer refusal. The log lines are judged after the harness
/// stops, when all of its output has drained.
pub(crate) async fn send(caller: &Caller) -> ConformanceResult<Vec<SentRefusal>> {
    let mut sent = Vec::new();
    for index in 0..13 {
        let organization = case_organization(index);
        let refusal = if index < 7 {
            profile_refusals(&organization)?.remove(index)
        } else {
            further_refusals(&organization, OTHER_STATEMENT)?.remove(index - 7)
        };
        let status = caller.provision(&organization, &refusal.statement).await?;
        sent.push(SentRefusal {
            organization,
            refusal,
            status,
        });
    }
    let policy = fixture_policy();
    let other_person = Caller {
        owner: sign_assertion(
            &json!({"iss":policy.issuer,"aud":[policy.admin_audience],"email":"someone-else@example.test",
                "sub":"fixture-other","type":"app","nbf":NOW_SECONDS - 1,"exp":NOW_SECONDS + 3600}),
            FIXTURE_KID,
        )?,
        ..caller.clone()
    };
    let observed = other_person
        .post(
            "/public/admin/v1/conformance/organization-records",
            &json!({"organization":case_organization(0)}),
        )
        .await?;
    require(
        observed.0 == 401,
        "the conformance observer refuses another person's valid admin assertion",
        &observed,
    )?;
    Ok(sent)
}

/// Judge the drained harness lines against what was sent.
pub(crate) fn judge(sent: &[SentRefusal], lines: &[String]) -> ConformanceResult<()> {
    let mut wrong = Vec::new();
    for case in sent {
        let expected_status = if case.refusal.refusal == "OrganizationMismatch" {
            403
        } else {
            422
        };
        let marker = format!(
            "central organization statement refused: organization={} refusal=",
            case.organization
        );
        let logged: Vec<&str> = lines
            .iter()
            .filter_map(|line| line.split(&marker).nth(1))
            .collect();
        if case.status != expected_status || logged != vec![case.refusal.refusal] {
            wrong.push((case.refusal.case, case.status, logged));
        }
    }
    require(
        wrong.is_empty(),
        "each refused statement answers as before and leaves exactly one Worker log line naming its kind",
        &wrong,
    )?;
    let text = lines.join("\n");
    let leaked = sent.iter().any(|case| {
        case.refusal
            .statement
            .split('.')
            .filter(|segment| segment.len() > 8)
            .any(|segment| text.contains(segment))
            || text.contains(&stamps::digest(&case.refusal.statement))
    }) || ["stamp-a", "stamp-b", "stamp-z"]
        .iter()
        .any(|kid| text.contains(kid));
    require(
        !leaked,
        "no Worker log line carries statement bytes, segments, digests or key IDs",
        &leaked,
    )
}
