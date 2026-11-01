use crate::test_support::event_driver::{EventJournal, SurfaceSnapshot, record_draw};
use agent_collaboration_tui::{
    actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
    foundation::theme::{SurfaceRole, TextRole},
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, GroupId, InputUse, TargetId, TargetInputHandler, TargetSpec},
        hit_regions::{HitRegion, HitRegionKind},
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, SurfaceStackHandle, use_surface_stack},
        surface_stack::{
            OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId, SurfaceModel,
        },
    },
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
#[derive(Clone, Default)]
pub(crate) struct SceneHandle(Arc<Mutex<Option<SurfaceStackHandle>>>);
impl SceneHandle {
    pub(crate) fn get(&self) -> SurfaceStackHandle {
        self.0.lock().unwrap().unwrap()
    }
}

pub(crate) fn target_id(value: &str) -> TargetId {
    TargetId::new(value).unwrap()
}
fn action_id(value: &str) -> ActionId {
    ActionId::new(value).unwrap()
}
fn action(name: &str, key: &str, run: impl Fn(()) + Send + Sync + 'static) -> ActionDef {
    ActionDef {
        risk: agent_collaboration_tui::actions::ActionRisk::Ordinary,
        toolbar_label: None,
        id: action_id(name),
        title: "Action".into(),
        keys: vec![key.parse().unwrap()],
        availability: AvailabilityFn::default(),
        choice: None,
        placement: Placement::Toolbar,
        run: Handler::from(run),
    }
}
fn overlay(
    stack: SurfaceStackHandle,
    journal: EventJournal,
    kind: OverlayKind,
    left: u16,
) -> OverlaySpec {
    let group = GroupId::new("items").unwrap();
    let targets = ["first", "second"]
        .into_iter()
        .map(|name| {
            TargetSpec::new(target_id(name))
                .in_group(group.clone())
                .with_activation(action_id("pick"))
        })
        .collect();
    OverlaySpec {
        kind,
        model: SurfaceModel {
            focus: FocusModel::new(targets).unwrap(),
            actions: ActionRegistry::new(
                ScopeId::new("overlay").unwrap(),
                vec![action("pick", "Enter", move |()| {
                    journal.log_handler("pick");
                    stack.close_top();
                })],
            )
            .unwrap(),
            primary_action: None,
        },
        view: OverlayView::new(|| element!(OverlayBody).into_any()),
        layout: OverlayLayout::AtCells {
            left,
            top: 4,
            width: 18,
            height: 6,
        },
    }
}

#[derive(Props)]
pub(crate) struct SceneProps {
    #[iocraft(required)]
    pub(crate) journal: EventJournal,
    pub(crate) wide_glyph: String,
    pub(crate) handle: SceneHandle,
    pub(crate) overlay_left: Option<u16>,
}
#[component]
pub(crate) fn Scene(props: &SceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let text = hooks.use_state(|| "seed".to_owned());
    let details = hooks.use_state(|| 0usize);
    let quits = hooks.use_state(|| 0usize);
    let overlay_left = props.overlay_left.unwrap_or(20);
    let stack = use_surface_stack(&mut hooks, |stack| {
        let input = TargetInputHandler::new(move |event| {
            let mut text = text;
            match event {
                TerminalEvent::Key(key) if key.modifiers.is_empty() => {
                    if let KeyCode::Char(character) = key.code {
                        let mut next = text.read().clone();
                        next.push(character);
                        text.set(next);
                        return InputUse::Used;
                    }
                }
                TerminalEvent::BracketedPaste(value) => {
                    let mut next = text.read().clone();
                    next.push_str(&value);
                    text.set(next);
                    return InputUse::Used;
                }
                _ => {}
            }
            InputUse::Unused
        });
        let open_menu = action("menu", "F10", {
            let journal = props.journal.clone();
            move |()| {
                journal.log_handler("menu");
                stack
                    .open_overlay(overlay(
                        stack,
                        journal.clone(),
                        OverlayKind::Menu,
                        overlay_left,
                    ))
                    .unwrap();
            }
        });
        let open_dialog = action("dialog", "F2", {
            let journal = props.journal.clone();
            move |()| {
                journal.log_handler("dialog");
                stack
                    .open_overlay(overlay(
                        stack,
                        journal.clone(),
                        OverlayKind::Dialog,
                        overlay_left,
                    ))
                    .unwrap();
            }
        });
        let inspect = action("details", "Enter", {
            let journal = props.journal.clone();
            move |()| {
                let mut details = details;
                details.set(details.get() + 1);
                journal.log_handler("details");
            }
        });
        let other = action("other", "F4", {
            let journal = props.journal.clone();
            move |()| {
                let mut details = details;
                details.set(details.get() + 1);
                journal.log_handler("other");
            }
        });
        let quit = action("quit", "Ctrl+C", move |()| {
            let mut quits = quits;
            quits.set(quits.get() + 1)
        });
        SurfaceModel {
            focus: FocusModel::new(vec![
                TargetSpec::new(target_id("filter"))
                    .with_input(input)
                    .with_activation(action_id("details"))
                    .in_group(GroupId::new("screen-fields").unwrap()),
                TargetSpec::new(target_id("other"))
                    .with_activation(action_id("other"))
                    .in_group(GroupId::new("screen-fields").unwrap()),
            ])
            .unwrap(),
            actions: ActionRegistry::new(
                ScopeId::new("screen").unwrap(),
                vec![open_menu, open_dialog, inspect, other, quit],
            )
            .unwrap(),
            primary_action: None,
        }
    });
    *props.handle.0.lock().unwrap() = Some(stack);
    let journal = props.journal.clone();
    element! {
        InputOwner(stack, width:60u16, height:18u16, quit_action: action_id("quit"), on_routed: move |_| {
            let state = stack.snapshot();
            let focus=state.surface(SurfaceId::Screen).unwrap().focus.current().map_or("-",TargetId::as_str);
            let mut surfaces = vec![(SurfaceId::Screen, format!("{}|{}|{}|{}", text.read().as_str(),details.get(),quits.get(),focus).into_bytes())];
            surfaces.extend(state.overlays().iter().map(|overlay| (SurfaceId::Overlay(overlay.id), overlay.model.focus.current().map_or_else(Vec::new, |id|id.as_str().as_bytes().to_vec()))));
            journal.processed(SurfaceSnapshot(surfaces));
        }) {
            OverlayHost {
                ScreenBody(text:text.read().clone(), details: details.get(), quits:quits.get(), wide_glyph:props.wide_glyph.clone())
            }
        }
    }
}

#[derive(Default, Props)]
struct ScreenBodyProps {
    text: String,
    details: usize,
    quits: usize,
    wide_glyph: String,
}
#[component]
fn ScreenBody(props: &ScreenBodyProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let theme = context.theme();
    let state = context.stack.snapshot();
    let controls=state.surface(SurfaceId::Screen).unwrap().focus.targets().iter().enumerate().map(|(index,target)| {
        let color=theme.foreground(if !target.is_enabled() {TextRole::Disabled}else if context.is_focused(&target.id) {TextRole::Focused}else{TextRole::Text});
        let content=if target.id.as_str()=="filter" {props.text.clone()}else{"Other".to_owned()};
        element!(HitRegion(target:target.id.clone(),kind:HitRegionKind::Target) {
            View(position:Position::Absolute,left:2,top:u16::try_from(3+index*3).unwrap(),width:12,height:1) {Text(content,color)}
        }).into_any()
    }).collect::<Vec<_>>();
    element! {
        View(width:100pct,height:100pct,flex_direction:FlexDirection::Column) {
            Text(content:format!("Screen {} {}",props.details,props.quits),color:theme.foreground(TextRole::Text))
            #(controls)
            View(position:Position::Absolute,left:0,top:11,column_gap:1) {
                #([TextRole::Text,TextRole::Muted,TextRole::Accent,TextRole::Focused,TextRole::Primary,TextRole::Disabled,TextRole::Dimmed,TextRole::Good,TextRole::Warning,TextRole::Error]
                    .into_iter().map(|role|element!(Text(content:"x",color:theme.foreground(role)))))
            }
            View(position:Position::Absolute,left:0,top:13,column_gap:1) {
                #([SurfaceRole::Base,SurfaceRole::Raised,SurfaceRole::GroupPanel,SurfaceRole::Header,SurfaceRole::Selected,SurfaceRole::Alternate]
                    .into_iter().map(|role|element!(View(width:1,height:1,background_color:theme.background(role)){Text(content:"b",color:theme.foreground(TextRole::Text))})))
            }
            #(if props.wide_glyph.is_empty() { None } else { Some(element! {
                View(position:Position::Absolute,left:19,top:4,flex_direction:FlexDirection::Column) {
                    #((0..6).map(|_| element!(Text(content:props.wide_glyph.clone(),color:theme.foreground(TextRole::Text)))))
                }
            }.into_any()) })
        }
    }
}

#[component]
fn OverlayBody(hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let theme = context.theme();
    element! {
        View(width:100pct,height:100pct,border_style:BorderStyle::Single,
            border_color:theme.foreground(TextRole::Accent),background_color:theme.background(SurfaceRole::Raised),flex_direction:FlexDirection::Column) {
            #( ["first","second"].into_iter().map(|name| element! {
                HitRegion(target:target_id(name),kind:HitRegionKind::Target) {
                    Text(content:format!("{} {name}",if context.is_focused(&target_id(name)) {"❯"} else {" "}),color:theme.foreground(TextRole::Text))
                }
            }))
        }
    }
}
