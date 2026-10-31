//! Interactive account composition; the static renderer keeps its own format.
use super::quota_fact_presentation::reset_pace_meter;
use super::quota_typography::{QuotaText, QuotaTextRole};
use agent_collaboration_tui::controls::SelectableRow;
use agent_collaboration_tui::{UiColumnHeading, UiText, UiTextRole};
use codex_router_quota_service::quota_display_model::{
    QuotaDisplayWindow, QuotaStatusAccountViewModel,
};
use iocraft::prelude::*;

/// The four useful minima decide folding, rather than fixed account percentages.
pub(crate) fn group_widths(width: usize) -> Option<[usize; 4]> {
    let surplus = width.checked_sub(85)?;
    let mut widths = [17, 29, 18, 18];
    let [account, quota, pace, extra] = &mut widths;
    let mut weighted_groups = [(24, account), (32, quota), (22, pace), (17, extra)];
    for (weight, allocated) in &mut weighted_groups {
        **allocated += surplus / 95 * *weight + surplus % 95 * *weight / 95;
    }
    // Largest remainders allocate the last cells without favoring one group.
    let assigned = weighted_groups
        .iter()
        .map(|(_, allocated)| **allocated)
        .sum::<usize>();
    let unassigned = width - 3 - assigned;
    weighted_groups.sort_by_key(|(weight, _)| std::cmp::Reverse(surplus % 95 * *weight % 95));
    for (_, allocated) in weighted_groups.into_iter().take(unassigned) {
        *allocated += 1;
    }
    Some(widths)
}

pub(crate) fn render_group_headers(width: usize) -> Option<AnyElement<'static>> {
    let widths = group_widths(width)?;
    Some(element! {
        View(column_gap:1,flex_shrink:0.0_f32) {
            #(["Account","Quota left · reset in","Weekly pace","Extra"].into_iter().zip(widths).map(|(content,width)|element! {
                View(width:u32::try_from(width).unwrap_or(u32::MAX),flex_shrink:0.0_f32) {UiColumnHeading(content)}
            }))
        }
    }.into_any())
}

fn reset_duration(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = seconds % 86400 / 3600;
    let minutes = seconds % 3600 / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

fn render_window(
    window: &QuotaDisplayWindow,
    label: &str,
    role: QuotaTextRole,
) -> AnyElement<'static> {
    match window {
        QuotaDisplayWindow::Unknown => element!(View).into_any(),
        QuotaDisplayWindow::Known {
            remaining_percent,
            reset_in_seconds,
            stale,
        } => {
            let filled = ((*remaining_percent).min(100) as usize * 7 / 100).min(7);
            let meter = format!("{}{}", "█".repeat(filled), "░".repeat(7 - filled));
            let reset = reset_in_seconds
                .map(|seconds| format!(" · {}", reset_duration(seconds)))
                .unwrap_or_default();
            element! {
                View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
                    QuotaText(content:format!("{meter} {remaining_percent}% · {label}{reset}"),role,wrap:TextWrap::Wrap)
                    #((*stale).then(||element!(UiText(content:format!("{label} stale"),role:UiTextRole::Warning))))
                }
            }.into_any()
        }
    }
}

fn account_group(row: &QuotaStatusAccountViewModel, focused: bool) -> AnyElement<'static> {
    let provider = match row.provider {
        codex_router_core::provider::Provider::Openai => "◎ OpenAI",
        codex_router_core::provider::Provider::Claude => "◆ Claude",
    };
    element! {
        View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
            QuotaText(content:row.account.clone(),role:QuotaTextRole::AccountName {focused},weight:Weight::Bold,wrap:TextWrap::Wrap)
            UiText(content:format!("{provider} · {}",row.active_clients),role:UiTextRole::Secondary,wrap:TextWrap::Wrap)
            UiText(content:row.status.compact(usize::MAX),role:UiTextRole::Body,wrap:TextWrap::Wrap)
        }
    }.into_any()
}

fn extra_group(row: &QuotaStatusAccountViewModel) -> AnyElement<'static> {
    use codex_router_core::credit_usage::CreditAvailability;
    let credits = match row.credit_usage.provider_observation.availability() {
        CreditAvailability::Available {
            balance: Some(balance),
        } => format!(
            "{} credits",
            codex_router_quota_service::credit_usage_summary::compact_credit_balance(balance)
        ),
        CreditAvailability::Available { balance: None } => "Credits available".to_owned(),
        CreditAvailability::Unlimited => "Unlimited credits".to_owned(),
        CreditAvailability::Depleted => "Credits depleted".to_owned(),
        CreditAvailability::Unknown => String::new(),
    };
    element! {
        View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
            #((!credits.is_empty()).then(||element!(UiText(content:credits,role:UiTextRole::Secondary,wrap:TextWrap::Wrap))))
            #((row.credit_usage.freshness==codex_router_quota_service::quota::CreditUsageFreshness::Stale).then(||element!(UiText(content:"Credits stale",role:UiTextRole::Warning))))
            #((row.reset_credits!="resets unknown").then(||element!(UiText(content:row.reset_credits.clone(),role:UiTextRole::Secondary,wrap:TextWrap::Wrap))))
        }
    }.into_any()
}

pub(crate) fn render_interactive_account(
    row: &QuotaStatusAccountViewModel,
    width: usize,
    focused: bool,
) -> AnyElement<'static> {
    element!(View(width:u32::try_from(width).unwrap_or(u32::MAX),flex_shrink:0.0_f32) {
        SelectableRow(selected:focused,marker_offset:if group_widths(width.saturating_sub(2)).is_some(){0u16}else{2u16}) {
            #(render_account_groups(row,width.saturating_sub(2),focused))
        }
    }).into_any()
}

pub(crate) fn render_account_groups(
    row: &QuotaStatusAccountViewModel,
    width: usize,
    focused: bool,
) -> AnyElement<'static> {
    let widths = group_widths(width);

    let pace = &row.reset_pace;
    let pace_label = pace.impact_label.clone().unwrap_or_else(|| {
        if pace.semantic_label == "burn unavailable" {
            "⏳"
        } else {
            pace.semantic_label
        }
        .to_owned()
    });
    let pace_detail = if pace.state
        == codex_router_quota_service::quota_display_model::ResetPaceState::Unavailable
    {
        let unavailable =
            codex_router_quota_service::quota_display_model::ResetPaceViewModel::default();
        pace.unavailable_reason
            .as_ref()
            .filter(|reason| Some(reason.as_str()) != unavailable.unavailable_reason.as_deref())
            .cloned()
            .or_else(|| {
                (pace.multiple_label != unavailable.multiple_label)
                    .then(|| pace.multiple_label.clone())
            })
    } else {
        Some(pace.multiple_label.clone())
    }
    .filter(|detail| detail != &pace_label);
    let groups=vec![
        ("Account",account_group(row,focused)),
        ("Quota left · reset in",element!(View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
            #(render_window(&row.display_weekly,"week",QuotaTextRole::WeeklyWindow))
            #(render_window(&row.display_short,"5h",QuotaTextRole::ShortWindow))
        }).into_any()),
        ("Weekly pace",element!(View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
            QuotaText(content:pace_label,role:QuotaTextRole::WeeklyPace(pace.state),wrap:TextWrap::Wrap)
            #(pace_detail.map(|detail|element!(QuotaText(content:detail,role:QuotaTextRole::Secondary,wrap:TextWrap::Wrap))))
            #((pace.state != codex_router_quota_service::quota_display_model::ResetPaceState::Unavailable).then(||element! {
                View(flex_shrink:0.0_f32) {QuotaText(content:reset_pace_meter(pace),role:QuotaTextRole::WeeklyPace(pace.state),wrap:TextWrap::NoWrap)}
            }))
        }).into_any()),
        ("Extra",extra_group(row)),
    ];
    element! {
        View(flex_direction:if widths.is_some() {FlexDirection::Row}else {FlexDirection::Column},column_gap:1,flex_shrink:0.0_f32) {
            #(groups.into_iter().enumerate().map(|(index,(label,group))|element! {
                View(width:u32::try_from(widths.and_then(|widths|widths.get(index).copied()).unwrap_or(width)).unwrap_or(u32::MAX),min_width:0,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column) {
                    #(widths.is_none().then(||element!(UiColumnHeading(content:label))))
                    #(group)
                }
            }))
        }
    }.into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_cells_follow_reference_weights_and_keep_minima_threshold() {
        assert_eq!(group_widths(0), None);
        assert_eq!(group_widths(84), None);
        for (width, expected) in [
            (85, [17, 29, 18, 18]),
            (86, [17, 30, 18, 18]),
            (120, [26, 41, 26, 24]),
            (159, [36, 54, 35, 31]),
            (160, [36, 54, 35, 32]),
        ] {
            let actual = group_widths(width).unwrap();
            assert_eq!(actual, expected, "reference 24:32:22:17 surplus at {width}");
            assert_eq!(
                actual.iter().sum::<usize>() + 3,
                width,
                "three one-cell gaps"
            );
        }
    }

    #[test]
    fn wide_actual_headers_use_literal_distributed_origins() {
        assert!(render_group_headers(84).is_none());
        for width in [159, 160] {
            let canvas = render_group_headers(width).unwrap().render(None);
            assert_eq!(canvas.width(), width);
            let frame = canvas.to_string();
            let heading = frame.lines().next().unwrap();
            for (label, origin) in [
                ("Account", 0),
                ("Quota left", 37),
                ("Weekly pace", 92),
                ("Extra", 128),
            ] {
                let before = heading.split_once(label).unwrap().0;
                assert_eq!(
                    unicode_width::UnicodeWidthStr::width(before),
                    origin,
                    "{width}: {frame}"
                );
            }
        }
    }
}
