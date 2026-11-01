//! Qualifies the shared runner against real Button input and serialized states.
//! These are two crate composition profiles, not either consumer's screen proof.
use super::{
    control_kind_contract::*,
    event_driver::{EventJournal, SurfaceSnapshot},
    frame_grid::FrameStyle,
};
use crate::surfaces::focus_model::TargetId;
use crate::{
    actions::{
        ActionDef, ActionId, ActionRegistry, ActionRisk, Availability, AvailabilityFn, Checked,
        Placement, ScopeId,
    },
    controls::fields::{Button, button_target},
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::FocusModel,
        overlay_host::OverlayHost,
        surface_handle::use_surface_stack,
        surface_stack::{SurfaceId, SurfaceModel},
    },
    test_support::event_driver::record_draw,
};
use iocraft::prelude::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn action(name: &str) -> ActionId {
    ActionId::new(name).unwrap()
}
fn target(name: &str) -> TargetId {
    TargetId::for_action(&action(name))
}
fn key(code: KeyCode) -> ControlInput {
    ControlInput::Event(TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code)))
}
fn snapshot(count: usize, enabled: bool) -> SurfaceSnapshot {
    SurfaceSnapshot(vec![(
        SurfaceId::Screen,
        format!("{count}|{enabled}").into_bytes(),
    )])
}
fn definition(
    name: &str,
    title: &'static str,
    keys: Vec<crate::foundation::KeyChord>,
    run: Handler<()>,
) -> ActionDef {
    ActionDef {
        risk: ActionRisk::Ordinary,
        toolbar_label: None,
        id: action(name),
        title: title.into(),
        keys,
        placement: Placement::Hidden,
        availability: AvailabilityFn::default(),
        choice: None,
        run,
    }
}

#[derive(Props)]
struct ContractButtonProps {
    #[iocraft(required)]
    journal: EventJournal,
    #[iocraft(required)]
    observed: ObservedControlStack,
    #[iocraft(required)]
    label: &'static str,
    width: u16,
    height: u16,
    duplicate_label: bool,
    initial_count: usize,
}
#[component]
fn ContractButton(props: &ContractButtonProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let count = hooks.use_state(|| props.initial_count);
    let enabled = hooks.use_state(|| true);
    let stack = use_surface_stack(&mut hooks, |_| {
        let journal = props.journal.clone();
        let mut run = definition(
            "run",
            props.label,
            vec!["F2".parse().unwrap()],
            Handler::from(move |()| {
                let mut count = count;
                count.set(count.get() + 1);
                journal.log_handler("run");
            }),
        );
        run.choice = Some(Checked::new(move || count.get() > 0));
        run.availability = AvailabilityFn::new(move || {
            if enabled.get() {
                Availability::Available
            } else {
                Availability::Unavailable {
                    reason: "Read only".into(),
                }
            }
        });
        let other = definition("other", "Other", Vec::new(), Handler::default());
        let disable = definition(
            "disable",
            "Disable",
            vec!["F4".parse().unwrap()],
            Handler::from(move |()| {
                let mut enabled = enabled;
                enabled.set(false);
            }),
        );
        let enable = definition(
            "enable",
            "Enable",
            vec!["F3".parse().unwrap()],
            Handler::from(move |()| {
                let mut enabled = enabled;
                enabled.set(true);
            }),
        );
        SurfaceModel {
            focus: FocusModel::new(vec![
                button_target(target("other"), &other),
                button_target(target("run"), &run),
            ])
            .unwrap(),
            actions: ActionRegistry::new(
                ScopeId::new("contract").unwrap(),
                vec![other, run, disable, enable],
            )
            .unwrap(),
            primary_action: None,
        }
    });
    props.observed.observer()(stack);
    let journal = props.journal.clone();
    element!(InputOwner(stack,width:props.width,height:props.height,quit_action:action("quit"),on_routed:move |_|journal.processed(snapshot(count.get(),enabled.get()))) {
        OverlayHost {View(flex_direction:FlexDirection::Column) {
            Button(id:target("other"),action:action("other"))
            Button(id:target("run"),action:action("run"))
            Text(content:format!("Calls {}",count.get()))
            #(props.duplicate_label.then(|| element!(Text(content:props.label,color:Color::Red))))
        }}
    })
}

#[path = "control_target_style_test.rs"]
mod target_style_test;

fn style(focused: bool, hovered: bool, checked: bool) -> FrameStyle {
    FrameStyle {
        foreground: Some(if focused || checked {
            Color::AnsiValue(11)
        } else {
            Color::AnsiValue(15)
        }),
        background: Some(if hovered {
            Color::Rgb {
                r: 48,
                g: 54,
                b: 65,
            }
        } else {
            Color::Rgb {
                r: 39,
                g: 44,
                b: 52,
            }
        }),
        bold: focused || checked,
        underline: focused,
        ..FrameStyle::default()
    }
}
fn painted(label: &str, focused: bool, hovered: bool, checked: bool) -> Vec<ControlPaint> {
    vec![ControlPaint::Styled {
        text: label.to_owned(),
        style: style(focused, hovered, checked),
    }]
}
fn contract(label: &'static str, viewport: (u16, u16)) -> ControlKindContract {
    ControlKindContract {
        screen: if label == "Start" {
            "picker composition profile"
        } else {
            "quota composition profile"
        },
        kind: ControlKind::Button,
        viewport,
        initial_paint: painted(label, false, false, false),
        steps: vec![
            ControlStep {
                input: key(KeyCode::Tab),
                redraw: true,
                expected_snapshot: snapshot(0, true),
                expected_handlers: Vec::new(),
                paint: painted(label, true, false, false),
            },
            ControlStep {
                input: ControlInput::Pointer {
                    target: target("run"),
                    kind: MouseEventKind::Moved,
                },
                redraw: true,
                expected_snapshot: snapshot(0, true),
                expected_handlers: Vec::new(),
                paint: painted(label, true, true, false),
            },
            ControlStep {
                input: key(KeyCode::Enter),
                redraw: true,
                expected_snapshot: snapshot(1, true),
                expected_handlers: vec!["run".into()],
                paint: painted(label, true, true, true),
            },
            ControlStep {
                input: key(KeyCode::F(4)),
                redraw: true,
                expected_snapshot: snapshot(1, false),
                expected_handlers: vec!["run".into()],
                paint: vec![ControlPaint::Contains("Read only".into())],
            },
            ControlStep {
                input: ControlInput::Pointer {
                    target: target("run"),
                    kind: MouseEventKind::Down(MouseButton::Left),
                },
                redraw: false,
                expected_snapshot: snapshot(1, false),
                expected_handlers: vec!["run".into()],
                paint: vec![ControlPaint::Contains("Read only".into())],
            },
            ControlStep {
                input: key(KeyCode::F(2)),
                redraw: false,
                expected_snapshot: snapshot(1, false),
                expected_handlers: vec!["run".into()],
                paint: vec![
                    ControlPaint::Contains("Calls 1".into()),
                    ControlPaint::Absent("Calls 2".into()),
                ],
            },
            ControlStep {
                input: key(KeyCode::F(3)),
                redraw: true,
                expected_snapshot: snapshot(1, true),
                expected_handlers: vec!["run".into()],
                paint: vec![ControlPaint::Absent("Read only".into())],
            },
            ControlStep {
                input: ControlInput::Pointer {
                    target: target("run"),
                    kind: MouseEventKind::Down(MouseButton::Left),
                },
                redraw: true,
                expected_snapshot: snapshot(2, true),
                expected_handlers: vec!["run".into(), "run".into()],
                paint: vec![ControlPaint::Contains("Calls 2".into())],
            },
        ],
    }
}
async fn run(contract: ControlKindContract, label: &'static str) {
    let (width, height) = contract.viewport;
    assert_control_kind_contract(contract, move |journal, observed| {
        element!(ContractButton(journal, observed, label, width, height))
    })
    .await;
}

#[tokio::test]
async fn both_composition_profiles_share_key_pointer_and_visual_state_contracts_at_all_grids() {
    for viewport in crate::test_support::grids::STANDARD_GRIDS {
        for label in ["Start", "Refresh"] {
            run(contract(label, viewport), label).await;
        }
    }
}

#[tokio::test]
#[should_panic(expected = "outcome:")]
async fn contract_rejects_wrong_callback_state_before_waiting_for_its_paint() {
    let mut case = contract("Start", (76, 30));
    case.steps[2].expected_snapshot = snapshot(99, true);
    run(case, "Start").await;
}

#[tokio::test]
#[should_panic(expected = "handlers:")]
async fn contract_rejects_a_missing_handler_observation() {
    let mut case = contract("Start", (76, 30));
    case.steps[2].expected_handlers.clear();
    run(case, "Start").await;
}

#[tokio::test]
#[should_panic(expected = "style for")]
async fn contract_rejects_wrong_serialized_visual_state() {
    let mut case = contract("Start", (76, 30));
    case.initial_paint = painted("Start", true, false, false);
    run(case, "Start").await;
}

#[tokio::test]
#[should_panic(expected = "paint \"Invented label\"")]
async fn contract_rejects_a_missing_visual_label() {
    let mut case = contract("Start", (76, 30));
    case.initial_paint = vec![ControlPaint::Contains("Invented label".into())];
    run(case, "Start").await;
}

#[tokio::test]
#[should_panic(expected = "R15 matrix missing TextField")]
async fn matrix_rejects_an_omitted_control_kind_before_mounting_a_substitute() {
    let mut screen = button_matrix_contracts();
    screen.kinds.retain(|declaration| {
        !matches!(
            declaration,
            ControlKindDeclaration::NotOnScreen {
                kind: ControlKind::TextField,
                ..
            }
        )
    });
    assert_control_kind_matrix::<AnyElement<'static>>(vec![screen], |_, _, _| {
        panic!("an undeclared kind must fail before mounting")
    })
    .await;
}

#[tokio::test]
#[should_panic(expected = "R15 matrix missing standard grid")]
async fn matrix_rejects_an_omitted_standard_grid_before_mounting_a_substitute() {
    let mut screen = button_matrix_contracts();
    for declaration in &mut screen.kinds {
        if let ControlKindDeclaration::OnScreen { contracts, .. } = declaration {
            contracts.retain(|contract| contract.viewport != (32, 24));
        }
    }
    assert_control_kind_matrix::<AnyElement<'static>>(vec![screen], |_, _, _| {
        panic!("an incomplete grid matrix must not mount a substitute")
    })
    .await;
}

const BUTTON_ONLY_REASON: &str = "This runner qualification scene mounts Buttons only.";
fn button_matrix_contracts() -> ScreenControlContracts {
    ScreenControlContracts {
        screen: "picker composition profile",
        kinds: ControlKind::ALL
            .into_iter()
            .map(|kind| {
                if kind == ControlKind::Button {
                    ControlKindDeclaration::OnScreen {
                        kind,
                        contracts: crate::test_support::grids::STANDARD_GRIDS
                            .into_iter()
                            .map(|viewport| contract("Start", viewport))
                            .collect(),
                    }
                } else {
                    ControlKindDeclaration::NotOnScreen {
                        kind,
                        reason: BUTTON_ONLY_REASON,
                    }
                }
            })
            .collect(),
    }
}

#[tokio::test]
async fn matrix_reports_absent_kinds_separately_and_runs_only_real_contracts_at_all_grids() {
    let mounts = Arc::new(AtomicUsize::new(0));
    let observed_mounts = mounts.clone();
    let summary = assert_control_kind_matrix(vec![button_matrix_contracts()],move |request,journal,observed| {
        observed_mounts.fetch_add(1,Ordering::SeqCst);
        assert_eq!(request.kind,ControlKind::Button);
        assert_eq!(request.screen,"picker composition profile");
        element!(ContractButton(journal,observed,label:"Start",width:request.viewport.0,height:request.viewport.1))
    }).await;
    assert_eq!(
        mounts.load(Ordering::SeqCst),
        4,
        "absent kinds never mount a substitute"
    );
    assert_eq!(
        summary.passed_contracts, 4,
        "only real completed contracts count as passes"
    );
    assert_eq!(summary.not_on_screen.len(), 7);
    assert!(
        summary
            .not_on_screen
            .iter()
            .all(|absent| absent.screen == "picker composition profile"
                && absent.kind != ControlKind::Button
                && absent.reason == BUTTON_ONLY_REASON)
    );
}

#[tokio::test]
async fn matrix_with_only_absent_declarations_reports_zero_runtime_passes() {
    let screen = ScreenControlContracts {
        screen: "empty qualification scene",
        kinds: ControlKind::ALL
            .into_iter()
            .map(|kind| ControlKindDeclaration::NotOnScreen {
                kind,
                reason: "This qualification scene mounts no controls.",
            })
            .collect(),
    };
    let summary = assert_control_kind_matrix::<AnyElement<'static>>(vec![screen], |_, _, _| {
        panic!("an absent kind must never mount")
    })
    .await;
    assert_eq!(summary.passed_contracts, 0);
    assert_eq!(summary.not_on_screen.len(), 8);
}

#[tokio::test]
#[should_panic(expected = "R15 NotOnScreen needs a reason")]
async fn matrix_rejects_an_absent_kind_with_an_empty_reason() {
    let mut screen = button_matrix_contracts();
    for declaration in &mut screen.kinds {
        if let ControlKindDeclaration::NotOnScreen {
            kind: ControlKind::FilterField,
            reason,
        } = declaration
        {
            *reason = " \n\t";
        }
    }
    assert_control_kind_matrix::<AnyElement<'static>>(vec![screen], |_, _, _| {
        panic!("empty absence reason must fail before mounting")
    })
    .await;
}

#[tokio::test]
#[should_panic(expected = "R15 matrix duplicates Button")]
async fn matrix_rejects_a_kind_declared_both_present_and_absent() {
    let mut screen = button_matrix_contracts();
    screen.kinds.push(ControlKindDeclaration::NotOnScreen {
        kind: ControlKind::Button,
        reason: "Contradicts the real Button contracts.",
    });
    assert_control_kind_matrix::<AnyElement<'static>>(vec![screen], |_, _, _| {
        panic!("duplicate kind must fail before mounting")
    })
    .await;
}

#[tokio::test]
#[should_panic(expected = "R15 real contract belongs to a different kind")]
async fn matrix_rejects_a_real_case_bound_to_the_wrong_kind() {
    let mut screen = button_matrix_contracts();
    for declaration in &mut screen.kinds {
        if let ControlKindDeclaration::OnScreen { contracts, .. } = declaration {
            contracts[0].kind = ControlKind::FilterField;
        }
    }
    assert_control_kind_matrix::<AnyElement<'static>>(vec![screen], |_, _, _| {
        panic!("misbound real kind must fail before mounting")
    })
    .await;
}
