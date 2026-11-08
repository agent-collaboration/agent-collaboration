//! Statement time and key-cache cases on workerd, each on its own Fleet
//! object: a statement must still be valid when its applying transaction runs,
//! and a retired key stops verifying once the per-object key cache's refresh
//! bound passes.
use super::control;
use super::link_client::Caller;
use super::organization_stamps::{self as stamps, NOW_SECONDS, SignInKeys};
use super::record_checks::{require, summary};
use collaboration_central_conformance::{
    ConformanceResult, DispatchCallSelector, DispatchKey, RuntimeControl,
};
use serde_json::{Value, json};

const EXPIRY_FIRST: &str = "019f0000-0000-7000-8000-00000000b2c8";
const EXPIRY_CONTROL: &str = "019f0000-0000-7000-8000-00000000b2c9";
const EXPIRY_STORED: &str = "019f0000-0000-7000-8000-00000000b2ca";
const WARM: &str = "019f0000-0000-7000-8000-00000000b2cb";
const WARM_CONTROL: &str = "019f0000-0000-7000-8000-00000000b2cc";
/// Stored under a setup that expires before a held, different setup does.
const STORED_EXPIRY: &str = "019f0000-0000-7000-8000-00000000b2cd";
const STORED_CONTROL: &str = "019f0000-0000-7000-8000-00000000b2ce";

async fn set_time_millis(
    caller: &Caller,
    organization: &str,
    millis: i64,
) -> ConformanceResult<()> {
    control(
        caller,
        organization,
        RuntimeControl::SetTime {
            now: collaboration_central_core::RegistryTime(
                chrono::DateTime::from_timestamp_millis(millis).ok_or("fixture time")?,
            ),
        },
    )
    .await
}

/// Hold `held`'s provisioning after it verifies, move its Fleet's clock to
/// `now_millis`, then release it into its applying transaction.
async fn held_across(
    caller: &Caller,
    organization: &str,
    held: String,
    now_millis: i64,
) -> ConformanceResult<u16> {
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
    control(
        caller,
        organization,
        RuntimeControl::AwaitHeldDispatch { key: key.clone() },
    )
    .await?;
    set_time_millis(caller, organization, now_millis).await?;
    control(caller, organization, RuntimeControl::ResumeDispatch { key }).await?;
    Ok(held_call.await??)
}

/// A renewal of `predecessor` that expires `shorter_by` seconds before it.
fn short_renewal(
    organization: &str,
    predecessor: &str,
    shorter_by: i64,
) -> ConformanceResult<String> {
    let mut claims = stamps::claims(organization);
    stamps::set_claim(&mut claims, "iat", json!(NOW_SECONDS - 50))?;
    stamps::set_claim(
        &mut claims,
        "exp",
        json!(NOW_SECONDS - 60 + stamps::LIFETIME_SECONDS - shorter_by),
    )?;
    stamps::set_claim(&mut claims, "replaces", json!(stamps::digest(predecessor)))?;
    stamps::sign(&stamps::stamp_header(), &claims, 7)
}

/// A setup statement, valid now, that expires `shorter_by` seconds before the
/// default statement does.
fn short_setup(organization: &str, shorter_by: i64) -> ConformanceResult<String> {
    let mut claims = stamps::claims(organization);
    stamps::set_claim(
        &mut claims,
        "exp",
        json!(NOW_SECONDS - 60 + stamps::LIFETIME_SECONDS - shorter_by),
    )?;
    stamps::sign(&stamps::stamp_header(), &claims, 7)
}

fn unprovisioned(records: &Value) -> bool {
    records.get("statement") == Some(&Value::Null) && summary(records).is_empty()
}

pub(crate) async fn run(caller: &Caller, keys: &SignInKeys) -> ConformanceResult<()> {
    let exp_millis = (NOW_SECONDS - 60 + stamps::LIFETIME_SECONDS) * 1000;

    let refused = held_across(
        caller,
        EXPIRY_FIRST,
        stamps::stamp(EXPIRY_FIRST)?,
        exp_millis,
    )
    .await?;
    let refused_rows = caller.records(EXPIRY_FIRST).await?;
    require(
        refused == 422 && unprovisioned(&refused_rows),
        "a first provision released into its transaction at its exp is refused: no stamp, no record",
        &(refused, refused_rows),
    )?;
    let control_answer = held_across(
        caller,
        EXPIRY_CONTROL,
        stamps::stamp(EXPIRY_CONTROL)?,
        exp_millis - 1000,
    )
    .await?;
    let control_digest = caller.digest(EXPIRY_CONTROL).await?;
    let control_rows = caller.records(EXPIRY_CONTROL).await?;
    require(
        control_answer == 200 && control_digest.0 == 200 && summary(&control_rows).len() == 1,
        "released one second before exp: provisioned and readable",
        &(control_answer, control_digest, control_rows),
    )?;

    let original = stamps::stamp(EXPIRY_STORED)?;
    let stored = caller.provision(EXPIRY_STORED, &original).await?;
    let before = caller.records(EXPIRY_STORED).await?;
    let renewal = held_across(
        caller,
        EXPIRY_STORED,
        short_renewal(EXPIRY_STORED, &original, 1_000)?,
        exp_millis - 1_000_000,
    )
    .await?;
    let after_renewal = caller.records(EXPIRY_STORED).await?;
    let replay = held_across(caller, EXPIRY_STORED, original, exp_millis).await?;
    let after_replay = caller.records(EXPIRY_STORED).await?;
    require(
        stored == 200
            && renewal == 422
            && replay == 422
            && after_renewal == before
            && after_replay == before,
        "a renewal (predecessor still valid) or a byte-identical replay released at its exp is refused and changes nothing",
        &(stored, renewal, replay, before, after_replay),
    )?;

    // Provisioned means a usable stored stamp at apply time: a different valid
    // setup held while the stored stamp expires gets the not-trusted refusal.
    let stored_exp_millis = exp_millis - 1_000_000;
    let mut stored_cases = Vec::new();
    for (organization, released_at) in [
        (STORED_EXPIRY, stored_exp_millis),
        (STORED_CONTROL, stored_exp_millis - 1000),
    ] {
        let provisioned = caller
            .provision(organization, &short_setup(organization, 1_000)?)
            .await?;
        let before = caller.records(organization).await?;
        let answer = held_across(
            caller,
            organization,
            stamps::stamp(organization)?,
            released_at,
        )
        .await?;
        let after = caller.records(organization).await?;
        stored_cases.push((provisioned, answer, after == before));
    }
    require(
        stored_cases == vec![(200, 409, true), (200, 200, true)],
        "a different setup held while the stored stamp expires is refused and changes nothing; released one second before, Provisioned and unchanged",
        &stored_cases,
    )?;

    // Warm cache, one Fleet object: K verifies, K is retired, it still verifies
    // at t0 + 299 999 ms and is refused from t0 + 300 000 ms.
    let t0 = NOW_SECONDS * 1000;
    set_time_millis(caller, WARM, t0).await?;
    let first = stamps::stamp_under_second_key_at(WARM, 0)?;
    let before_warm = keys.fetches();
    let cached = caller.provision(WARM, &first).await?;
    let warmed = keys.fetches() - before_warm;
    keys.retire("stamp-b")?;
    set_time_millis(caller, WARM, t0 + 299_999).await?;
    let inside = caller
        .provision(WARM, &stamps::stamp_under_second_key_at(WARM, 1)?)
        .await?;
    let inside_fetches = keys.fetches() - before_warm;
    set_time_millis(caller, WARM, t0 + 300_000).await?;
    let past = caller
        .provision(WARM, &stamps::stamp_under_second_key_at(WARM, 2)?)
        .await?;
    let boundary_fetches = keys.fetches() - before_warm;
    let over_refused = caller.provision(WARM, &stamps::stamp(WARM)?).await?;
    let remaining = caller
        .provision(WARM_CONTROL, &stamps::stamp(WARM_CONTROL)?)
        .await?;
    let warm_rows = caller.records(WARM).await?;
    require(
        (cached, inside, past, over_refused, remaining) == (200, 200, 422, 409, 200)
            && (warmed, inside_fetches, boundary_fetches) == (1, 1, 3)
            && warm_rows.get("statement") == Some(&json!(first))
            && summary(&warm_rows).len() == 1,
        "a warm key cache refuses a retired key once its 300 s refresh bound passes, and nothing else changes",
        &(
            (cached, inside, past, over_refused, remaining),
            (warmed, inside_fetches, boundary_fetches),
            warm_rows,
        ),
    )?;
    Ok(())
}
