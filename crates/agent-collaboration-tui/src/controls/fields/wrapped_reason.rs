//! Shared availability text uses its allocated width and contributes all wrapped rows.
use iocraft::prelude::*;
#[derive(Props)]
pub(crate) struct WrappedReasonProps {
    pub content: String,
    #[iocraft(required)]
    pub color: Color,
}
#[component]
pub(crate) fn WrappedReason(props: &WrappedReasonProps) -> impl Into<AnyElement<'static>> {
    element!(View(width:100pct,flex_shrink:0.0_f32) {Text(content:props.content.clone(),wrap:TextWrap::Wrap,color:props.color)})
}
