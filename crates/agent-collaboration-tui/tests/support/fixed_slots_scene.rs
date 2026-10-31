use super::event_driver::{EventJournal, SurfaceSnapshot};
use agent_collaboration_tui::{
    DialogAxisSize, DialogMargins, DialogPresentation,
    actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
    controls::{
        Button, DialogFrame, PinnedContext, ScrollHandle, SegmentedSwitch, button_target,
        segmented_targets,
    },
    focus_model::{FocusModel, GroupId, TargetId, TargetSpec},
    input_owner::InputOwner,
    overlay_host::OverlayHost,
    surface_handle::{SurfaceStackHandle, use_surface_stack},
    surface_stack::{OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceModel},
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};

pub fn target(name: &str) -> TargetId {
    TargetId::new(name).unwrap()
}
pub fn action(name: &str) -> ActionId {
    ActionId::new(name).unwrap()
}
#[derive(Clone)]
pub struct FixedSlotsFixture {
    pub stack: SurfaceStackHandle,
    pub scroll: ScrollHandle,
}
#[derive(Props)]
pub struct FixedSlotsSceneProps {
    #[iocraft(required)]
    pub journal: EventJournal,
    pub width: u16,
    pub height: u16,
    pub rows: u16,
    pub maximum_height: Option<u16>,
    pub content_height: bool,
    pub controls: bool,
    pub status: bool,
    pub tall_slots: bool,
    pub handle: Arc<Mutex<Option<FixedSlotsFixture>>>,
    pub calls: Arc<Mutex<Vec<String>>>,
}
#[component]
pub fn FixedSlotsScene(
    props: &FixedSlotsSceneProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let status_visible = hooks.use_state(|| props.status);
    let stack = use_surface_stack(&mut hooks, |_| SurfaceModel {
        focus: FocusModel::new(Vec::new()).unwrap(),
        actions: ActionRegistry::new(ScopeId::new("fixed-screen").unwrap(), Vec::new()).unwrap(),
        primary_action: None,
    });
    let mut opened = hooks.use_ref(|| false);
    if !opened.get() {
        let scroll = ScrollHandle::new(stack);
        *props.handle.lock().unwrap() = Some(FixedSlotsFixture {
            stack,
            scroll: scroll.clone(),
        });
        let definitions = [
            ("header", "Help", "F1"),
            ("resets", "Resets", "F2"),
            ("credits", "Credits", "F3"),
            ("close", "Close", "Esc"),
            ("toggle-status", "Toggle status", "F8"),
        ]
        .into_iter()
        .map(|(name, title, key)| {
            let calls = props.calls.clone();
            ActionDef {
                id: action(name),
                title: title.into(),
                toolbar_label: None,
                keys: vec![key.parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: if name == "close" {
                    Placement::Toolbar
                } else {
                    Placement::Hidden
                },
                run: Handler::from(move |()| {
                    if name == "toggle-status" {
                        let mut visible = status_visible;
                        visible.set(!visible.get());
                    } else {
                        calls.lock().unwrap().push(name.to_owned());
                    }
                }),
            }
        })
        .collect::<Vec<_>>();
        let mut targets = vec![button_target(
            TargetId::for_action(&action("header")),
            &definitions[0],
        )];
        if props.controls {
            targets.extend(segmented_targets(
                &definitions[1..3],
                GroupId::new("pinned-segments").unwrap(),
                move || stack.viewport().0.saturating_sub(6),
            ));
        }
        targets.push(TargetSpec::new(target("body")).with_input(scroll.input_handler()));
        targets.push(button_target(
            TargetId::for_action(&action("close")),
            &definitions[3],
        ));
        let rows = props.rows;
        let controls = props.controls;
        let tall_slots = props.tall_slots;
        let maximum = props
            .maximum_height
            .unwrap_or(props.height.saturating_sub(2));
        stack.open_overlay(OverlaySpec {
            kind:OverlayKind::Dialog,
            model:SurfaceModel {focus:FocusModel::new(targets).unwrap(),actions:ActionRegistry::new(ScopeId::new("fixed-dialog").unwrap(),definitions).unwrap(),primary_action:None},
            view:OverlayView::new(move || element!(FixedSlotsPanel(scroll:scroll.clone(),rows,controls,status:status_visible.get(),tall_slots,width:stack.viewport().0.saturating_sub(6))).into_any()),
            layout:OverlayLayout::Dialog {presentation:DialogPresentation::Sized {
                width:DialogAxisSize::cells(props.width.saturating_sub(4)),
                height:if props.content_height {DialogAxisSize::Content {maximum:Some(maximum)}}else{DialogAxisSize::cells(maximum)},
            },margins:DialogMargins {top:1,right:2,bottom:1,left:2}},
        }).unwrap();
        opened.set(true);
    }
    let journal = props.journal.clone();
    element!(InputOwner(stack,width:props.width,height:props.height,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(Vec::new()))) {
        OverlayHost {Text(content:"Fixed slot fixture")}
    })
}
#[derive(Props)]
struct FixedSlotsPanelProps {
    #[iocraft(required)]
    scroll: ScrollHandle,
    width: u16,
    rows: u16,
    controls: bool,
    status: bool,
    tall_slots: bool,
}
#[component]
fn FixedSlotsPanel(props: &FixedSlotsPanelProps) -> impl Into<AnyElement<'static>> {
    let mut controls=props.controls.then(||element!(SegmentedSwitch(actions:vec![action("resets"),action("credits")],available_width:props.width)).into_any()).into_iter().collect::<Vec<_>>();
    if props.controls && props.tall_slots {
        controls.push(element!(Text(content:"Controls detail")).into_any());
    }
    let status = props.status.then(|| {
        element!(View(flex_direction:FlexDirection::Column) {
            Text(content:"Credit balance refreshed.")
            #(props.tall_slots.then(||element!(Text(content:"Status detail"))))
        })
        .into_any()
    });
    element!(DialogFrame(title:"Options · Resets".to_owned(),scroll:props.scroll.clone(),scroll_target:target("body"),padding:Some(0),context:Some(PinnedContext::new("東京").with_tag("alpha-tag")),help_action:Some(element!(Button(id:TargetId::for_action(&action("header")),action:action("header"))).into_any()),pinned_controls:controls,pinned_status:status) {
        #((0..props.rows).map(|row|element!(View(height:1,flex_shrink:0.0_f32) {Text(content:format!("BODY_{row:02}"))})))
    })
}
