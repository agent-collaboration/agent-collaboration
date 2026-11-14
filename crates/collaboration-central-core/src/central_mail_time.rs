//! The portable timestamp range supported by both central alarm hosts.
use workspace_model::Timestamp;
/// Last integer millisecond accepted by pinned workerd's signed nanosecond clock.
/// Proven by `platform_alarm_accepts_exact_boundary_and_rejects_later_milliseconds`;
/// original receipt: tmp/2026-11-14-b3/platform-range-probe.log (1 passed, exit0).
/// Pinned source: cloudflare/workerd v1.20261012.1, jsg/value.h:1218–1224.
pub const MAX_ALARM_UNIX_MILLIS: i64 = 9_223_372_036_854;
pub(crate) fn supported(time: Timestamp) -> bool {
    (-MAX_ALARM_UNIX_MILLIS..=MAX_ALARM_UNIX_MILLIS).contains(&time.timestamp_millis())
}
pub(crate) fn refusal(
    operation: &collaboration_protocol::central_mail::CentralMailOperation,
    now: Timestamp,
) -> Option<String> {
    use collaboration_protocol::central_mail::CentralMailOperation as Op;
    if !supported(now) {
        return Some("anchorAt".to_owned());
    }
    match operation {
        Op::SetWake { due_at, .. } if !supported(*due_at) => Some("dueAt".to_owned()),
        Op::SetAutomation { timing, .. } => {
            if let agent_automation::TimingRule::At { at } = timing
                && !supported(*at)
            {
                return Some("timing.at".to_owned());
            }
            if timing
                .next_due(now, None)
                .ok()
                .flatten()
                .is_some_and(|due| !supported(due))
            {
                Some("nextDueAt".to_owned())
            } else {
                None
            }
        }
        _ => None,
    }
}
