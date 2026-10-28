//! Read-only whole-pool facts. Account effects and focus remain caller-owned.
use super::quota_status_symbols::QuotaHeaderFacts;
use codex_router_tui::{
    DialogAction, DialogFrame, DialogMargins, DialogScrollKeys, UiHeading, UiHeadingLevel, UiText,
    UiTextRole,
};
use iocraft::prelude::*;

pub(super) fn render_pool_details(
    width: u16,
    height: u16,
    header: &QuotaHeaderFacts,
    degraded: bool,
    mut details: State<Option<super::quota_dialog_rendering::QuotaDetailsDialog>>,
) -> AnyElement<'static> {
    let pool = header.pool;
    let freshness = header.freshness;
    let facts = [
        ("Accounts", pool.total()),
        ("Usable", pool.usable),
        ("Reserve", pool.reserve),
        ("Blocked", pool.blocked),
        ("Pool unknown", pool.unknown),
        ("Excluded", pool.excluded),
        ("Fresh", freshness.fresh),
        ("Stale", freshness.stale),
        ("Data unknown", freshness.unknown),
    ];
    element! {
        DialogFrame(width,height,title:"Pool details",margins:DialogMargins {top:1,right:2,bottom:1,left:2},scroll_keys:DialogScrollKeys::Navigation,
            actions:vec![element!(DialogAction(label:"Close",handler:move |_|details.set(None))).into_any()]) {
            UiHeading(content:header.route_band.clone(),level:UiHeadingLevel::Section)
            #(facts.into_iter().map(|(label,count)|element!(Text(content:format!("{label}: {count}"),color:Color::White,wrap:TextWrap::Wrap))))
            Text(content:format!("Clients: {}",header.observed_clients.map_or_else(||"unknown".to_owned(),|count|count.to_string())),color:Color::White,wrap:TextWrap::Wrap)
            #(header.credential_notice.clone().map(|content|element!(UiText(content,role:UiTextRole::Error,wrap:TextWrap::Wrap))))
            #(degraded.then(||element!(UiText(content:"Selection projection degraded",role:UiTextRole::Error,wrap:TextWrap::Wrap))))
        }
    }.into_any()
}
