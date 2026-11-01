//! Content-height fixtures use heterogeneous public children with independently countable rows.
use super::sample_model::{action, target_id};
use agent_collaboration_tui::{
    DialogAxisSize, DialogMargins, DialogPresentation, UiText, UiTextRole,
    actions::{ActionDef, ActionRegistry, Placement, ScopeId},
    controls::list::ScrollHandle,
    dialog::DialogFrame,
    surfaces::focus_model::{FocusModel, TargetSpec},
    surfaces::surface_handle::SurfaceStackHandle,
    surfaces::surface_stack::{OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceModel},
};
use iocraft::prelude::*;

#[derive(Clone, Copy)]
enum IntrinsicScene {
    OneLine,
    GappedLines,
    NestedContent,
}
#[expect(
    clippy::expect_used,
    reason = "Fixed sample identities fail visibly if the public overlay refuses them."
)]
pub fn intrinsic_actions(stack: SurfaceStackHandle) -> Vec<ActionDef> {
    [
        (
            "intrinsic-one",
            "Intrinsic one row",
            IntrinsicScene::OneLine,
        ),
        (
            "intrinsic-four",
            "Intrinsic four rows",
            IntrinsicScene::GappedLines,
        ),
        (
            "intrinsic-eight",
            "Intrinsic eight rows",
            IntrinsicScene::NestedContent,
        ),
    ]
    .into_iter()
    .map(|(name, title, scene)| {
        action(
            name,
            title,
            &[],
            Placement::Menu {
                section: "Intrinsic content",
            },
            move |()| {
                let scroll = ScrollHandle::new(stack);
                let close = action("close", "Close", &["Esc"], Placement::Hidden, move |()| {
                    stack.close_top();
                });
                let focus = FocusModel::new(vec![
                    TargetSpec::new(target_id("intrinsic-scroll"))
                        .pointer_only()
                        .with_input(scroll.input_handler()),
                ])
                .expect("intrinsic focus");
                let actions = ActionRegistry::new(
                    ScopeId::new("intrinsic-content").expect("scope"),
                    vec![close],
                )
                .expect("intrinsic actions");
                let view = OverlayView::new(move || {
                    element!(IntrinsicContent(scene,scroll:scroll.clone())).into_any()
                });
                stack
                    .open_overlay(OverlaySpec {
                        kind: OverlayKind::Dialog,
                        model: SurfaceModel {
                            focus,
                            actions,
                            primary_action: None,
                        },
                        view,
                        layout: OverlayLayout::Dialog {
                            presentation: DialogPresentation::Sized {
                                width: DialogAxisSize::cells(28),
                                height: DialogAxisSize::Content { maximum: Some(30) },
                            },
                            margins: DialogMargins {
                                top: 1,
                                right: 2,
                                bottom: 1,
                                left: 2,
                            },
                        },
                    })
                    .expect("intrinsic overlay");
            },
        )
    })
    .collect()
}
#[derive(Props)]
struct IntrinsicContentProps {
    #[iocraft(required)]
    scene: IntrinsicScene,
    #[iocraft(required)]
    scroll: ScrollHandle,
}
#[component]
fn IntrinsicContent(props: &IntrinsicContentProps) -> impl Into<AnyElement<'static>> {
    let body = match props.scene {
        IntrinsicScene::OneLine => {
            element!(UiText(content:"First and Last".to_owned(),role:UiTextRole::Body)).into_any()
        }
        IntrinsicScene::GappedLines => {
            element!(View(flex_direction:FlexDirection::Column,flex_shrink:0.0_f32,row_gap:1) {
                UiText(content:"First\nSecond".to_owned(),role:UiTextRole::Body)
                UiText(content:"Last".to_owned(),role:UiTextRole::Body)
            })
            .into_any()
        }
        IntrinsicScene::NestedContent => {
            element!(View(flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
                UiText(content:"First\nSecond\nThird".to_owned(),role:UiTextRole::Body)
                View(height:2,flex_shrink:0.0_f32)
                View(flex_direction:FlexDirection::Column,flex_shrink:0.0_f32,row_gap:1) {
                    UiText(content:"Nested".to_owned(),role:UiTextRole::Body)
                    UiText(content:"Last".to_owned(),role:UiTextRole::Body)
                }
            })
            .into_any()
        }
    };
    element!(DialogFrame(title:"Intrinsic content".to_owned(),padding:Some(0u16),scroll:props.scroll.clone(),scroll_target:target_id("intrinsic-scroll")) {#(Some(body))}).into_any()
}
