//! On workerd: each refused organization statement leaves exactly one Worker
//! log line naming its closed kind and the Fleet's organization, a valid one
//! leaves none, no line carries statement material, and the conformance
//! observer refuses a valid assertion from a person who is not the owner.
//!
//! `InvalidConfiguration` is not sent here: central produces it when its
//! sign-in settings are missing or unusable, and this journey's central is
//! configured. The Tokio refusal-log test drives that path on a link opened
//! with an empty issuer.
use super::link_client::Caller;
use super::organization_stamps::{self as stamps, NOW_SECONDS, SignInKeys};
use super::record_checks::require;
use super::statement_refusals::{StatementRefusal, further_refusals, profile_refusals};
use collaboration_central_conformance::{
    ConformanceResult, FIXTURE_KID, fixture_policy, sign_assertion,
};
use serde_json::json;

/// Names the statement used by the another-organization case.
const OTHER_STATEMENT: &str = "019f0000-0000-7000-8000-00000000b3af";
/// Offered a statement once the sign-in key source is down.
const UNREACHABLE: &str = "019f0000-0000-7000-8000-00000000b3ae";
/// The `Debug` prefix of a refusal whose key source could not answer.
const UNAVAILABLE: &str = "VerificationUnavailable(";

/// A refusal sent to its own organization, and the answer it drew.
pub(crate) struct SentRefusal {
    organization: String,
    refusal: StatementRefusal,
    status: u16,
}

fn case_organization(index: usize) -> String {
    format!("019f0000-0000-7000-8000-00000000b3{:02x}", 0xa0 + index)
}

/// The refusal kinds the Worker logged for `organization`, in order.
pub(crate) fn logged<'a>(lines: &'a [String], organization: &str) -> Vec<&'a str> {
    let marker =
        format!("central organization statement refused: organization={organization} refusal=");
    lines
        .iter()
        .filter_map(|line| line.split(&marker).nth(1))
        .collect()
}

/// Require exactly the listed refusal kinds per organization (none for a
/// valid case), judged on drained lines.
pub(crate) fn expect_logged(
    lines: &[String],
    expected: &[(&str, &[&str])],
    case: &str,
) -> ConformanceResult<()> {
    let wrong: Vec<_> = expected
        .iter()
        .map(|(organization, kinds)| (*organization, logged(lines, organization), *kinds))
        .filter(|(_, found, kinds)| found.as_slice() != *kinds)
        .collect();
    require(wrong.is_empty(), case, &wrong)
}

/// Send every refusal case, each to a never-named organization, check the
/// non-owner observer refusal, then take the key source down and offer one
/// more. The log lines are judged after the harness stops, when all of its
/// output has drained.
pub(crate) async fn send(caller: &Caller, keys: SignInKeys) -> ConformanceResult<Vec<SentRefusal>> {
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
    // A key ID no cache holds forces a refresh the stopped source can't answer.
    keys.shutdown().await?;
    let statement = stamps::sign(
        &json!({"typ":"fleet-organization-stamp+jwt","alg":"EdDSA","kid":"stamp-y"}),
        &stamps::claims(UNREACHABLE),
        9,
    )?;
    let status = caller.provision(UNREACHABLE, &statement).await?;
    sent.push(SentRefusal {
        organization: UNREACHABLE.to_owned(),
        refusal: StatementRefusal {
            case: "key source unavailable",
            statement,
            refusal: UNAVAILABLE,
        },
        status,
    });
    Ok(sent)
}

/// Judge the drained harness lines against what was sent.
pub(crate) fn judge(sent: &[SentRefusal], lines: &[String]) -> ConformanceResult<()> {
    let mut wrong = Vec::new();
    for case in sent {
        let expected_status = match case.refusal.refusal {
            "OrganizationMismatch" => 403,
            UNAVAILABLE => 503,
            _ => 422,
        };
        let found = logged(lines, &case.organization);
        let named = match found.as_slice() {
            [only] if case.refusal.refusal == UNAVAILABLE => only.starts_with(UNAVAILABLE),
            [only] => *only == case.refusal.refusal,
            _ => false,
        };
        if case.status != expected_status || !named {
            wrong.push((case.refusal.case, case.status, found));
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
    }) || ["stamp-a", "stamp-b", "stamp-y", "stamp-z"]
        .iter()
        .any(|kid| text.contains(kid));
    require(
        !leaked,
        "no Worker log line carries statement bytes, segments, digests or key IDs",
        &leaked,
    )
}
