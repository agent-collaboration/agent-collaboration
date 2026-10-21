//! Render the caller's captured label without owning or changing action identity.
use iocraft::prelude::*;

pub(super) fn render_session_context(label: &str, session_id: Option<&str>) -> AnyElement<'static> {
    let label = label.to_owned();
    let hint = session_id
        .map(|session_id| format!("[{}]", super::picker_component::fit_line(session_id, 8)));
    element! {View(width:100pct,max_width:100pct,column_gap:1,flex_shrink:0.0_f32) {
        View(min_width:0,flex_grow:1.0_f32,flex_shrink:1.0_f32,overflow:Overflow::Hidden) {Text(content:label,color:Color::White,wrap:TextWrap::NoWrap)}
        #(hint.map(|content|element!(View(flex_shrink:0.0_f32) {Text(content,color:Color::Grey,wrap:TextWrap::NoWrap)})))
    }}.into_any()
}
