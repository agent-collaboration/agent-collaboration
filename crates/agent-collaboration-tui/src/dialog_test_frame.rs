//! The existing static layout assertions exercise the new chrome without duplicating it.
use crate::{
    DialogMargins, DialogPresentation,
    actions::{ActionId, ActionRegistry, ScopeId},
    controls::ScrollHandle,
    focus_model::{FocusModel, TargetId},
    input_owner::InputOwner,
    surface_handle::{SurfaceContext, use_surface_stack},
    surface_stack::{
        OverlayKind, OverlayLayout, OverlayPlacement, OverlaySpec, OverlayView, SurfaceId,
        SurfaceModel,
    },
};
use iocraft::prelude::*;

#[derive(Props)]
pub struct StaticDialogFrameProps<'a> {
    pub presentation: DialogPresentation,
    #[iocraft(required)]
    pub width: u16,
    #[iocraft(required)]
    pub height: u16,
    #[iocraft(required)]
    pub title: String,
    pub margins: DialogMargins,
    pub padding: Option<u16>,
    pub border_style: Option<BorderStyle>,
    pub border_color: Option<Color>,
    pub help_action: Option<AnyElement<'a>>,
    pub context: Option<crate::controls::PinnedContext>,
    pub children: Vec<AnyElement<'a>>,
    pub actions: Vec<AnyElement<'a>>,
    pub action_hints: Vec<AnyElement<'a>>,
}
#[component]
pub fn StaticDialogFrame<'a>(
    props: &mut StaticDialogFrameProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let model = || SurfaceModel {
        focus: FocusModel::new(Vec::new()).unwrap(),
        actions: ActionRegistry::new(ScopeId::new("layout").unwrap(), Vec::new()).unwrap(),
        primary_action: None,
    };
    let stack = use_surface_stack(&mut hooks, |_| model());
    let scroll = hooks.use_ref(|| ScrollHandle::new(stack)).read().clone();
    let layout = OverlayLayout::Dialog {
        presentation: props.presentation,
        margins: props.margins,
    };
    let mut opened = hooks.use_ref(|| None);
    if opened.get().is_none() {
        opened.set(Some(
            stack
                .open_overlay(OverlaySpec {
                    kind: OverlayKind::Dialog,
                    model: model(),
                    view: OverlayView::default(),
                    layout: layout.clone(),
                })
                .unwrap(),
        ));
    }
    let surface = SurfaceId::Overlay(opened.get().unwrap());
    let OverlayPlacement::Centered {
        width,
        height,
        max_width,
        max_height,
        margins,
    } = layout.resolve((props.width, props.height))
    else {
        panic!("expected the declared dialog sizing variant")
    };
    let children = std::mem::take(&mut props.children);
    let actions = std::mem::take(&mut props.actions);
    let action_hints = std::mem::take(&mut props.action_hints);
    let help_action = props.help_action.take();
    let context = std::mem::take(&mut props.context);
    element!(View(position:Position::Absolute,top:0,left:0,width:props.width,height:props.height) {
        InputOwner(stack,width:props.width,height:props.height,quit_action:ActionId::new("quit").unwrap()) {
            View(width:100pct,height:100pct,padding_left:margins.left,padding_right:margins.right,padding_top:margins.top,padding_bottom:margins.bottom,align_items:AlignItems::Center,justify_content:JustifyContent::Center,overflow:Overflow::Hidden) {
                View(width,height,max_width,max_height,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column) {
                    ContextProvider(value:Context::owned(SurfaceContext {stack,surface})) {
                        ContextProvider(value:Context::owned(crate::overlay_host::SurfaceSizing {width,height,max_width,max_height})) {
                            crate::controls::DialogFrame(title:props.title.clone(),scroll,scroll_target:TargetId::new("body").unwrap(),padding:props.padding,border_style:props.border_style,border_color:props.border_color,children,actions,action_hints,help_action,context)
                        }
                    }
                }
            }
        }
    })
}
