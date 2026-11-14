//! Timing admission rejects invalid shapes and horizons before any durable effect.
use super::*;

pub(super) async fn exercise_refusals(f: &Fixture, target: &m::SessionAddress) {
    let runtime = f.host.central(&f.caller.organization_id).await;
    let snapshot = || Statement {
        sql: "SELECT (SELECT json_group_array(json_array(automation_id,target,timing_rule,anchor_at,expires_at,evaluated_through_at,text,next_due_at,state,created_at,updated_at)) FROM automations),(SELECT json_group_array(json_array(wake_id,target,due_at,text,state,created_at,updated_at)) FROM wakes),(SELECT json_group_array(json_array(message_id,sender,target_session,body,state,deliver_after,created_at,updated_at)) FROM messages),(SELECT json_group_array(json_array(actor_identity_id,identity,notice_cadence,last_notice_delivery_at,created_at,updated_at)) FROM actor_identities),(SELECT json_group_array(json_array(request_id,caller,content_digest,answer,created_at,updated_at)) FROM request_log)",
        args: vec![],
    };
    let before = runtime
        .statement_host()
        .read(snapshot())
        .await
        .unwrap()
        .rows;
    let alarm_before = runtime
        .control::<Option<i64>>("raw-alarm-time", &())
        .await
        .unwrap();
    for (index, (timing, field, requirement)) in [
        (
            serde_json::json!({"kind":"after","seconds":605_100}),
            "timing.seconds",
            TimingRequirement::WithinSevenDays,
        ),
        (
            serde_json::json!({"kind":"interval","seconds":605_100}),
            "timing.seconds",
            TimingRequirement::WithinSevenDays,
        ),
        (
            serde_json::json!({"kind":"after","seconds":31_622_700}),
            "timing.seconds",
            TimingRequirement::WithinSevenDays,
        ),
        (
            serde_json::json!({"kind":"interval","seconds":31_622_700}),
            "timing.seconds",
            TimingRequirement::WithinSevenDays,
        ),
        (
            serde_json::json!({"kind":"after","seconds":0}),
            "timing.seconds",
            TimingRequirement::FiveMinuteGrid,
        ),
        (
            serde_json::json!({"kind":"interval","seconds":0}),
            "timing.seconds",
            TimingRequirement::FiveMinuteGrid,
        ),
        (
            serde_json::json!({"kind":"after","seconds":299}),
            "timing.seconds",
            TimingRequirement::FiveMinuteGrid,
        ),
        (
            serde_json::json!({"kind":"interval","seconds":299}),
            "timing.seconds",
            TimingRequirement::FiveMinuteGrid,
        ),
        (
            serde_json::json!({"kind":"after","seconds":301}),
            "timing.seconds",
            TimingRequirement::FiveMinuteGrid,
        ),
        (
            serde_json::json!({"kind":"interval","seconds":301}),
            "timing.seconds",
            TimingRequirement::FiveMinuteGrid,
        ),
        (
            serde_json::json!({"kind":"cron","expression":"* * * * *","timezone":"UTC"}),
            "timing.expression",
            TimingRequirement::FiveMinuteGrid,
        ),
        (
            serde_json::json!({"kind":"cron","expression":"0,7 * * * *","timezone":"UTC"}),
            "timing.expression",
            TimingRequirement::FiveMinuteGrid,
        ),
        (
            serde_json::json!({"kind":"cron","expression":"0-5 * * * *","timezone":"UTC"}),
            "timing.expression",
            TimingRequirement::FiveMinuteGrid,
        ),
        (
            serde_json::json!({"kind":"at","at":now(f) + chrono::Duration::nanoseconds(1)}),
            "timing.at",
            TimingRequirement::FiveMinuteGrid,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let result = request(
            f,
            &f.caller,
            42200 + u32::try_from(index).unwrap(),
            Op::SetAutomation {
                target: target.clone(),
                timing: serde_json::from_value(timing).unwrap(),
                text: body(),
            },
        )
        .await;
        assert_eq!(
            result,
            Result::Refused(Refusal::TimingViolation {
                field: field.to_owned(),
                requirement,
            }),
            "admission case {index}"
        );
        assert_eq!(
            runtime
                .statement_host()
                .read(snapshot())
                .await
                .unwrap()
                .rows,
            before,
            "admission case {index} wrote durable state"
        );
        assert_eq!(
            runtime
                .control::<Option<i64>>("raw-alarm-time", &())
                .await
                .unwrap(),
            alarm_before,
            "admission case {index} changed the alarm"
        );
    }
}
