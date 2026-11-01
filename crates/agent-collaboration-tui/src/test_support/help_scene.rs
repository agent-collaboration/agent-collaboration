use crate::test_support::event_driver::{EventDriver, EventJournal, SurfaceSnapshot, record_draw};
use agent_collaboration_tui::{
    actions::{
        ActionDef, ActionId, ActionRegistry, Availability, AvailabilityFn, Placement, ScopeId,
    },
    controls::fields::{Button, button_target},
    dialog::help::{HelpContent, HelpControl},
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, TargetId},
        overlay_host::OverlayHost,
        surface_handle::{SurfaceStackHandle, use_surface_stack},
        surface_stack::{SurfaceId, SurfaceModel},
    },
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
pub fn action(name: &str) -> ActionId {
    ActionId::new(name).unwrap()
}
pub fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
pub fn text(driver: &EventDriver) -> String {
    let grid = driver.frames.last().unwrap();
    if grid.cells.iter().flatten().any(|cell| cell.text == "╭") {
        return grid.round_frame_text();
    }
    (0..grid.height)
        .map(|row| grid.row_text(row))
        .collect::<Vec<_>>()
        .join("\n")
}
#[derive(Clone, Default)]
pub struct HelpHandle(pub Arc<Mutex<Option<HelpFixture>>>);
#[derive(Clone)]
pub struct HelpFixture {
    pub control: HelpControl,
    pub enabled: State<bool>,
    pub stack: SurfaceStackHandle,
}
#[derive(Props)]
pub struct HelpSceneProps {
    #[iocraft(required)]
    pub journal: EventJournal,
    pub handle: HelpHandle,
    pub content: HelpContent,
}
pub const TITLES: [&str; 26] = [
    "Alpha action",
    "Bravo action",
    "Charlie action",
    "Delta action",
    "Echo action",
    "Foxtrot action",
    "Golf action",
    "Hotel action",
    "India action",
    "Juliet action",
    "Kilo action",
    "Lima action",
    "Mike action",
    "November action",
    "Oscar action",
    "Papa action",
    "Quebec action",
    "Romeo action",
    "Sierra action",
    "Tango action",
    "Uniform action",
    "Victor action",
    "Whiskey action",
    "Xray action",
    "Yankee action",
    "Zulu action",
];
#[component]
pub fn HelpScene(props: &HelpSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let enabled = hooks.use_state(|| true);
    let content = props.content.clone();
    let stack = use_surface_stack(&mut hooks, |stack| {
        let help = HelpControl::new(stack, action("help"));
        *props.handle.0.lock().unwrap() = Some(HelpFixture {
            control: help.clone(),
            enabled,
            stack,
        });
        let open = ActionDef {
            toolbar_label: None,
            id: action("help"),
            title: "Help".into(),
            keys: vec!["F1".parse().unwrap(), "Ctrl+7".parse().unwrap()],
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Toolbar,
            run: help.open_handler(
                move || content.clone(),
                Handler::from(|error| panic!("{error}")),
            ),
        };
        let execute = ActionDef {
            toolbar_label: None,
            id: action("execute"),
            title: "Execute".into(),
            keys: vec!["F8".parse().unwrap()],
            availability: AvailabilityFn::new(move || {
                if enabled.get() {
                    Availability::Available
                } else {
                    Availability::Unavailable {
                        reason: "Already executed".into(),
                    }
                }
            }),
            choice: None,
            placement: Placement::Toolbar,
            run: Handler::from(move |()| {
                let mut enabled = enabled;
                enabled.set(false);
            }),
        };
        let targets = vec![
            button_target(TargetId::for_action(&open.id), &open),
            button_target(TargetId::for_action(&execute.id), &execute),
        ];
        let mut actions = vec![open, execute];
        for (letter, title) in ('a'..='z').zip(TITLES) {
            actions.push(ActionDef {
                toolbar_label: None,
                id: action(title),
                title: title.into(),
                keys: vec![format!("Ctrl+Alt+{letter}").parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Menu { section: "Actions" },
                run: Handler::default(),
            });
        }
        SurfaceModel {
            focus: FocusModel::new(targets).unwrap(),
            actions: ActionRegistry::new(ScopeId::new("help-scene").unwrap(), actions).unwrap(),
            primary_action: None,
        }
    });
    let help = props
        .handle
        .0
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .control
        .clone();
    let journal = props.journal.clone();
    element!(InputOwner(stack,width:76u16,height:30u16,quit_action:action("quit"),on_routed:move |_|{let snapshot=stack.snapshot();let mut surfaces=vec![(SurfaceId::Screen,vec![u8::from(enabled.get())])];for overlay in snapshot.overlays(){surfaces.push((SurfaceId::Overlay(overlay.id),overlay.model.focus.current().unwrap().as_str().as_bytes().to_vec()));}journal.processed(SurfaceSnapshot(surfaces));}) {
        OverlayHost {View {Button(id:TargetId::for_action(help.action()),action:help.action().clone()) Button(id:TargetId::for_action(&action("execute")),action:action("execute"))}}
    })
}
