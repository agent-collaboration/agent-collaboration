//! Complete B2 provisioning/renewal metadata, through the private Unix link
//! and the owning modern store. Opaque comparisons never print signed bytes.
use super::support::*;
use central_storage::SqlValue;
use serde_json::{Value, json};
use std::time::Duration;

fn expected_metadata(compact: &str, issued_offset: i64) -> Value {
    json!({
        "digest":signed::digest(compact),
        "expiresAt":signed::NOW_SECONDS - 60 + issued_offset + signed::LIFETIME_SECONDS,
        "signingKeyId":"stamp-a"
    })
}

fn provisioning_record_matches(
    row: &[SqlValue],
    request_id: &str,
    kind: &str,
    at_ms: i64,
) -> Result<bool> {
    let uuid_v7 = match row.first() {
        Some(SqlValue::Text(id)) => {
            uuid::Uuid::parse_str(id).is_ok_and(|id| id.get_version_num() == 7)
        }
        _ => false,
    };
    Ok(uuid_v7
        && row.len() == 9
        && row.get(1) == Some(&text(request_id))
        && row.get(2) == Some(&text(kind))
        && row.get(3) == Some(&text("signInProvisioning"))
        && row.get(4) == Some(&SqlValue::Null)
        && row.get(5) == Some(&SqlValue::Null)
        && row.get(6) == Some(&text(A))
        && row.get(7) == Some(&number(at_ms)?)
        && row.get(8) == Some(&number(at_ms)?))
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Complete metadata and owning-row assertions fail without printing signed material."
)]
#[tokio::test]
async fn complete_stamp_metadata_and_record_identity_survive_renewal_and_setup_replays() -> Result {
    let f = Fixture::new().await?;
    let initial_at = signed::NOW_SECONDS * 1000;
    let first = signed::stamp(A, signed::NOW_SECONDS)?;
    let first_reply = f.provision(A, &first).await?;
    assert_eq!(first_reply.status, 200);
    assert!(first_reply.body == br#"{"result":"provisioned"}"#);
    let initial = f.snapshot().await?;
    assert_eq!((initial.0.len(), initial.1.len()), (1, 1));
    assert!(
        initial.0
            == vec![vec![
                text(A),
                text(&first),
                number(initial_at)?,
                number(initial_at)?
            ]],
        "the initial identity retains the exact compact and both applying timestamps"
    );
    let first_record = initial.1.first().ok_or("setup record missing")?;
    assert!(
        provisioning_record_matches(first_record, A, "organizationProvisioned", initial_at)?,
        "setup record identity, request, origin, target or timestamps differ"
    );
    let digest = f.digest(A).await?;
    assert_eq!(digest.status, 200);
    assert!(
        digest.value()? == expected_metadata(&first, 0),
        "initial digest metadata differs"
    );
    assert_eq!(f.provision(A, &first).await?.status, 200);
    assert!(
        f.snapshot().await? == initial,
        "an identical setup rewrote retained rows"
    );

    f.clock.advance(Duration::from_secs(10))?;
    let renewed_at = initial_at + 10_000;
    let renewal = signed::renewal(A, &first, signed::NOW_SECONDS, 10)?;
    assert_eq!(f.provision(A, &renewal).await?.status, 200);
    let renewed = f.snapshot().await?;
    assert_eq!((renewed.0.len(), renewed.1.len()), (1, 2));
    assert!(
        renewed.0
            == vec![vec![
                text(A),
                text(&renewal),
                number(initial_at)?,
                number(renewed_at)?
            ]],
        "replacement must retain the exact renewal and original creation time"
    );
    assert!(
        renewed.1.first() == Some(first_record),
        "renewal rewrote the setup record"
    );
    let renewal_record = renewed.1.get(1).ok_or("renewal record missing")?;
    assert!(
        provisioning_record_matches(
            renewal_record,
            &signed::digest(&renewal),
            "stampRenewed",
            renewed_at
        )?,
        "renewal record identity, request, origin, target or timestamps differ"
    );
    assert!(
        renewal_record.first() != first_record.first(),
        "renewal reused the setup record identity"
    );
    let digest = f.digest(A).await?;
    assert_eq!(digest.status, 200);
    let retained_metadata = digest.value()?;
    assert!(
        retained_metadata == expected_metadata(&renewal, 10),
        "renewal digest metadata differs"
    );

    let stale = signed::renewal(A, &first, signed::NOW_SECONDS, 11)?;
    let refused = f.provision(A, &stale).await?;
    assert_eq!(refused.status, 409);
    assert!(refused.body.is_empty());
    for replay in [&renewal, &first] {
        assert_eq!(f.provision(A, replay).await?.status, 200);
        assert!(
            f.snapshot().await? == renewed,
            "replay changed stamp or record identities/timestamps"
        );
        let digest = f.digest(A).await?;
        assert_eq!(digest.status, 200);
        assert!(
            digest.value()? == retained_metadata,
            "replay changed retained digest metadata"
        );
    }
    f.close().await
}
