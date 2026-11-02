use crate::{
    actions::{
        ActionDef, ActionId, ActionRegistry, ActionRisk, Availability, AvailabilityFn, Placement,
        ScopeId,
    },
    controls::{
        fields::{Button, TextField, TextFieldState, button_target},
        list::{
            ListRowKey, ListRows, ListSelectionBinding, ListSelectionControl, ListSelectionState,
        },
    },
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, TargetId, TargetSpec},
        surface_handle::{SurfaceContext, use_surface_stack},
        surface_stack::{SurfaceId, SurfaceModel},
    },
    test_support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot},
        recording_backend::recording_terminal_render_loop,
    },
};
use iocraft::prelude::*;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ActivationControl {
    #[default]
    Button,
    DisabledButton,
    List,
    TextField,
}
fn activate() -> ActionId {
    ActionId::new("activate").unwrap()
}
fn target() -> TargetId {
    TargetId::new("control").unwrap()
}

#[derive(Props)]
struct ActivationSceneProps {
    #[iocraft(required)]
    journal: EventJournal,
    control: ActivationControl,
}
#[component]
fn ActivationScene(
    props: &ActivationSceneProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let count = hooks.use_state(|| 0usize);
    let editor = hooks.use_state(|| TextFieldState::new(""));
    let selection = hooks.use_state(|| {
        ListSelectionState::new(
            ListRows::Ungrouped(vec![ListRowKey::new("row").unwrap()]),
            None,
        )
        .unwrap()
    });
    let control = props.control;
    let stack = use_surface_stack(&mut hooks, |stack| {
        let journal = props.journal.clone();
        let action = ActionDef {
            id: activate(),
            title: "Activate".into(),
            toolbar_label: None,
            risk: ActionRisk::Ordinary,
            placement: Placement::Toolbar,
            keys: Vec::new(),
            choice: None,
            availability: AvailabilityFn::new(move || {
                if control == ActivationControl::DisabledButton {
                    Availability::Unavailable {
                        reason: "disabled".into(),
                    }
                } else {
                    Availability::Available
                }
            }),
            run: Handler::from(move |()| {
                let mut count = count;
                count.set(count.get() + 1);
                journal.log_handler("activate");
            }),
        };
        let targets = match control {
            ActivationControl::Button | ActivationControl::DisabledButton => {
                vec![button_target(target(), &action)]
            }
            ActivationControl::List => ListSelectionControl::new(ListSelectionBinding {
                parent: SurfaceContext {
                    stack,
                    surface: SurfaceId::Screen,
                },
                id: target(),
                value: selection,
                on_select: Handler::default(),
                activation: activate(),
                page_rows: Arc::new(|| 3),
            })
            .targets(),
            ActivationControl::TextField => vec![
                TargetSpec::new(target())
                    .with_input(TextFieldState::input_handler(editor, Handler::default())),
            ],
        };
        SurfaceModel {
            focus: FocusModel::new(targets).unwrap(),
            actions: ActionRegistry::new(ScopeId::new("activation").unwrap(), vec![action])
                .unwrap(),
            primary_action: None,
        }
    });
    let journal = props.journal.clone();
    element!(InputOwner(stack,width:60u16,height:12u16,quit_action:ActionId::new("quit").unwrap(),on_routed:move |_| {
        journal.processed(SurfaceSnapshot(vec![(SurfaceId::Screen,format!("{}|{}",count.get(),editor.read().text()).into_bytes())]));
    }) { View(flex_direction:FlexDirection::Column) {
        #(match control {
            ActivationControl::Button | ActivationControl::DisabledButton => element!(Button(id:target(),action:activate())).into_any(),
            ActivationControl::List => element!(Text(content:"row")).into_any(),
            ActivationControl::TextField => element!(TextField(id:target(),value:editor)).into_any(),
        })
        Text(content:format!("calls={}",count.get()))
    }})
}

async fn assert_space_input(control: ActivationControl, mode: InputMode) {
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(ActivationScene(journal, control));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        driver.initial(&mut frames).await;
        let kinds = if control == ActivationControl::TextField {
            vec![KeyEventKind::Press]
        } else {
            vec![
                KeyEventKind::Press,
                KeyEventKind::Repeat,
                KeyEventKind::Release,
            ]
        };
        let steps = kinds
            .into_iter()
            .enumerate()
            .map(|(index, kind)| {
                (
                    TerminalEvent::Key(KeyEvent::new(kind, KeyCode::Char(' '))),
                    index == 0
                        && matches!(
                            control,
                            ActivationControl::Button | ActivationControl::TextField
                        ),
                )
            })
            .collect();
        let expected: &[u8] = match control {
            ActivationControl::Button => b"1|",
            ActivationControl::DisabledButton | ActivationControl::List => b"0|",
            ActivationControl::TextField => b"0| ",
        };
        let acknowledgements = driver
            .drive_checked(
                &mut frames,
                mode,
                steps,
                matches!(
                    control,
                    ActivationControl::Button | ActivationControl::TextField
                ),
                |ack| {
                    assert_eq!(ack.snapshot.0[0].1, expected, "{control:?} / {mode:?}");
                    assert_eq!(
                        ack.handlers.len(),
                        usize::from(control == ActivationControl::Button)
                    );
                },
            )
            .await;
        assert_eq!(
            acknowledgements.len(),
            if control == ActivationControl::TextField {
                1
            } else {
                3
            }
        );
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn space_activates_buttons_once_on_press_in_burst_and_after_frames() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        assert_space_input(ActivationControl::Button, mode).await;
    }
}
#[tokio::test]
async fn space_keeps_lists_inert_and_text_literal_and_disabled_buttons_inert() {
    for control in [
        ActivationControl::List,
        ActivationControl::TextField,
        ActivationControl::DisabledButton,
    ] {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            assert_space_input(control, mode).await;
        }
    }
}
