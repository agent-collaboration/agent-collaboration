use collaboration_protocol::{
    SubscriptionWaitBatch, ThreadSubscribeRequest, ThreadSubscriptionView,
    ThreadSubscriptionWaitRequest, ThreadSubscriptionWaitResult, ThreadSubscriptionsRequest,
    ThreadSubscriptionsResult, ThreadUnsubscribeRequest,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn ensure(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

#[test]
fn join_keeps_explicit_watch_and_accepts_optional_subscription_policy() -> TestResult {
    let mut join = mutation(
        json!({"thread":{"thread":"01900000-0000-7000-8000-000000000001"},
        "actor":actor(),"role":"participant","watch":true,"replace":null,"note":null}),
    );
    let decoded: message_board::ThreadJoinRequest = serde_json::from_value(join.clone())?;
    ensure(decoded.watch, "contract assertion failed")?;
    ensure(
        decoded.mode.is_none() && decoded.when_idle.is_none(),
        "omitted join policy did not default to None",
    )?;
    join["watch"] = json!(false);
    let decoded: message_board::ThreadJoinRequest = serde_json::from_value(join.clone())?;
    ensure(!decoded.watch, "explicit no-watch was lost")?;
    assert_round_trip::<message_board::ThreadJoinRequest>(&join)?;
    let mut missing_watch = join.clone();
    missing_watch
        .as_object_mut()
        .ok_or("join request is not an object")?
        .remove("watch");
    ensure(
        serde_json::from_value::<message_board::ThreadJoinRequest>(missing_watch).is_err(),
        "watch became optional",
    )?;
    join["mode"] = json!("poll");
    join["whenIdle"] = json!("wake");
    assert_round_trip::<message_board::ThreadJoinRequest>(&join)?;
    join["extra"] = json!(true);
    ensure(
        serde_json::from_value::<message_board::ThreadJoinRequest>(join).is_err(),
        "contract assertion failed",
    )?;
    Ok(())
}

fn mutation(fields: Value) -> Value {
    let mut value = json!({"requestId":message_board::RequestId::generate(),"protocolVersion":1});
    if let (Some(target), Some(source)) = (value.as_object_mut(), fields.as_object()) {
        target.extend(source.clone());
    }
    value
}
fn actor() -> Value {
    json!({"kind":"human", "humanId":"subscription-reader"})
}

fn scope() -> Value {
    json!({"kind":"thread", "threadId":"01900000-0000-7000-8000-000000000001"})
}

fn target() -> Value {
    json!({"kind":"thread", "thread":{"thread":"01900000-0000-7000-8000-000000000001"}})
}

fn ranges() -> Value {
    json!([{"threadId":"01900000-0000-7000-8000-000000000001",
        "projectId":"01900000-0000-7000-8000-000000000002",
        "from":{"epoch":1,"position":1}, "through":{"epoch":1,"position":2}, "postCount":2}])
}

fn assert_round_trip<TContract: Serialize + DeserializeOwned>(value: &Value) -> TestResult {
    let decoded: TContract = serde_json::from_value(value.clone())?;
    let encoded = serde_json::to_value(decoded)?;
    if encoded != *value {
        return Err(format!(
            "round-trip changed the wire shape: expected {value}, actual {encoded}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn subscription_requests_round_trip_and_reject_unknown_fields() -> TestResult {
    let subscribe = mutation(
        json!({"actingFor":null,"actor":actor(), "scope":target(), "policy":{
        "mode":"poll", "whenIdle":"hold", "quietSeconds":0,
        "capSeconds":600, "lifetimeSeconds":86400}}),
    );
    assert_round_trip::<ThreadSubscribeRequest>(&subscribe)?;
    assert_round_trip::<ThreadUnsubscribeRequest>(&mutation(
        json!({"actingFor":null,"actor":actor(), "scope":target()}),
    ))?;
    assert_round_trip::<ThreadSubscriptionsRequest>(&json!({"actor":actor()}))?;
    let mut unknown = subscribe.clone();
    unknown["extra"] = json!(true);
    ensure(
        serde_json::from_value::<ThreadSubscribeRequest>(unknown).is_err(),
        "contract assertion failed",
    )?;
    let mut unknown_policy = subscribe.clone();
    unknown_policy["policy"]["extra"] = json!(true);
    ensure(
        serde_json::from_value::<ThreadSubscribeRequest>(unknown_policy).is_err(),
        "contract assertion failed",
    )?;
    let mut unknown_scope = subscribe;
    unknown_scope["scope"]["extra"] = json!(true);
    ensure(
        serde_json::from_value::<ThreadSubscribeRequest>(unknown_scope).is_err(),
        "contract assertion failed",
    )?;
    ensure(
        serde_json::from_value::<ThreadUnsubscribeRequest>(mutation(
            json!({"actingFor":null,"actor":actor(), "scope":target(), "extra":true}),
        ))
        .is_err(),
        "contract assertion failed",
    )?;
    ensure(
        serde_json::from_value::<ThreadSubscriptionsRequest>(
            json!({"actor":actor(), "extra":true}),
        )
        .is_err(),
        "contract assertion failed",
    )?;
    Ok(())
}

#[test]
fn subscription_wait_accepts_bounds_and_rejects_overflow_and_unknown_fields() -> TestResult {
    for filter in [
        json!({"kind":"all"}),
        json!({"kind":"roots", "threads":[
        {"thread":"01900000-0000-7000-8000-000000000001"}]}),
        json!({"kind":"project", "projectId":"01900000-0000-7000-8000-000000000002"}),
    ] {
        for seconds in [0, 1500] {
            assert_round_trip::<ThreadSubscriptionWaitRequest>(&json!({
                "protocolVersion":1,"actingFor":null,"renewals":[],"actor":actor(), "filter":filter, "maxWaitSeconds":seconds}))?;
        }
    }
    for seconds in [1501, u64::MAX] {
        ensure(
            serde_json::from_value::<ThreadSubscriptionWaitRequest>(json!({
            "protocolVersion":1,"actingFor":null,"renewals":[],"actor":actor(), "filter":{"kind":"all"}, "maxWaitSeconds":seconds}))
            .is_err(),
            "contract assertion failed",
        )?;
    }
    ensure(
        serde_json::from_value::<ThreadSubscriptionWaitRequest>(json!({
        "protocolVersion":1,"actingFor":null,"renewals":[],"actor":actor(), "filter":{"kind":"all"}, "maxWaitSeconds":1, "extra":true}))
        .is_err(),
        "contract assertion failed",
    )?;
    ensure(
        serde_json::from_value::<ThreadSubscriptionWaitRequest>(json!({
        "protocolVersion":1,"actingFor":null,"renewals":[],"actor":actor(), "filter":{"kind":"all", "extra":true}, "maxWaitSeconds":1}))
        .is_err(),
        "contract assertion failed",
    )?;
    Ok(())
}

#[test]
fn subscription_wait_union_is_discriminated_and_body_free() -> TestResult {
    let notice = json!({"kind":"notice", "pushId":"01900000-0000-7000-8000-000000000003",
        "line":"🧵 Router: activity", "held":true, "heldSince":"2026-10-01T00:00:00Z",
        "draining":false, "roots":ranges()});
    let ranges = json!({"kind":"ranges", "held":false, "draining":true, "roots":ranges()});
    assert_round_trip::<SubscriptionWaitBatch>(&notice)?;
    assert_round_trip::<SubscriptionWaitBatch>(&ranges)?;
    assert_round_trip::<ThreadSubscriptionWaitResult>(
        &json!({"renewals":[],"outcome":{"kind":"batch","batch":notice}}),
    )?;
    assert_round_trip::<ThreadSubscriptionWaitResult>(
        &json!({"renewals":[],"outcome":{"kind":"batch","batch":ranges}}),
    )?;
    assert_round_trip::<ThreadSubscriptionWaitResult>(
        &json!({"renewals":[],"outcome":{"kind":"timedOut"}}),
    )?;
    for mut invalid in [notice.clone(), ranges.clone()] {
        invalid["body"] = json!("peer content");
        ensure(
            serde_json::from_value::<SubscriptionWaitBatch>(invalid).is_err(),
            "contract assertion failed",
        )?;
    }
    let mut invalid = notice;
    invalid["kind"] = json!("ranges");
    ensure(
        serde_json::from_value::<SubscriptionWaitBatch>(invalid).is_err(),
        "contract assertion failed",
    )?;
    let mut invalid = ranges;
    invalid["kind"] = json!("notice");
    ensure(
        serde_json::from_value::<SubscriptionWaitBatch>(invalid).is_err(),
        "contract assertion failed",
    )?;
    Ok(())
}

#[test]
fn subscription_view_round_trips_optional_observations() -> TestResult {
    let view = json!({"projectId":"01900000-0000-7000-8000-000000000002","scope":scope(), "policy":{"mode":"poll", "whenIdle":"hold",
        "timing":{"quietSeconds":120,"capSeconds":600},"lifetime":86400},
        "state":"active", "expiresAt":"2026-10-02T00:00:00Z", "pendingCount":2,
        "presence":{"kind":"unreachable","reason":"human reader"},
        "heldSince":"2026-10-01T00:00:00Z", "nextRetryAt":"2026-10-01T00:00:30Z",
        "lastOutcome":{"kind":"notSubmitted","reason":"idle","retryable":true}});
    assert_round_trip::<ThreadSubscriptionView>(&view)?;
    assert_round_trip::<ThreadSubscriptionsResult>(
        &json!({"subscriptions":[view],"unavailable":["01900000-0000-7000-8000-000000000003"]}),
    )?;
    ensure(
        serde_json::from_value::<ThreadSubscriptionsResult>(json!({"subscriptions":[]})).is_err(),
        "unavailable is required; old result shapes must be refused",
    )?;
    let mut unknown = view;
    unknown["extra"] = json!(true);
    ensure(
        serde_json::from_value::<ThreadSubscriptionView>(unknown).is_err(),
        "contract assertion failed",
    )?;
    for presence in [json!({"kind":"running"}), json!({"kind":"wakeable"})] {
        let mut observed = json!({"projectId":"01900000-0000-7000-8000-000000000002","scope":scope(), "policy":{"mode":"poll", "whenIdle":"hold",
            "timing":{"quietSeconds":120,"capSeconds":600},"lifetime":86400},
            "state":"ended", "endReason":{"kind":"cancelled"},
            "expiresAt":"2026-10-02T00:00:00Z", "pendingCount":0,"presence":presence});
        assert_round_trip::<ThreadSubscriptionView>(&observed)?;
        observed["presence"]["extra"] = json!(true);
        ensure(
            serde_json::from_value::<ThreadSubscriptionView>(observed).is_err(),
            "presence accepted unknown fields",
        )?;
    }
    Ok(())
}

#[test]
fn the_subscription_wait_schema_bounds_the_wait() -> TestResult {
    let request_schema =
        serde_json::to_value(schemars::schema_for!(ThreadSubscriptionWaitRequest))?;
    let validator = jsonschema::validator_for(&request_schema)?;
    let mut request = json!({"protocolVersion":1,"actingFor":null,"renewals":[],"actor":actor(),"filter":{"kind":"all"},"maxWaitSeconds":1500});
    ensure(validator.is_valid(&request), "contract assertion failed")?;
    request["maxWaitSeconds"] = json!(1501);
    ensure(!validator.is_valid(&request), "contract assertion failed")?;
    let serialized_schema = schemars::generate::SchemaSettings::draft2020_12()
        .for_serialize()
        .into_generator()
        .into_root_schema_for::<ThreadSubscriptionWaitRequest>();
    let serialized_validator =
        jsonschema::validator_for(&serde_json::to_value(serialized_schema)?)?;
    ensure(
        !serialized_validator.is_valid(&request),
        "serialized wait schema omitted the upper bound",
    )?;
    Ok(())
}
