use crate::test_support::{
    event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
    recording_backend::recording_terminal_render_loop,
};
use agent_collaboration_tui::{
    actions::{
        ActionDef, ActionId, ActionRegistry, Availability, AvailabilityFn, Placement, ScopeId,
    },
    controls::{
        combo::{ComboBox, ComboBoxBinding, ComboBoxControl, ComboBoxState},
        fields::{TextField, TextFieldState},
        list::{OptionKey, SelectOption},
    },
    foundation::theme::{SurfaceRole, TextRole, Theme},
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, TargetId, TargetSpec},
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, SurfaceStackHandle, use_surface_stack},
        surface_stack::{SurfaceId, SurfaceModel},
    },
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
#[derive(Clone, Copy)]
enum ControlKind {
    Combo,
    Editor,
}
#[derive(Clone, Copy)]
struct Fixture {
    stack: SurfaceStackHandle,
    enabled: State<bool>,
    text: State<TextFieldState>,
}
fn target(name: &str) -> TargetId {
    TargetId::new(name).unwrap()
}
fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
#[derive(Props)]
struct SceneProps {
    #[iocraft(required)]
    journal: EventJournal,
    #[iocraft(required)]
    kind: ControlKind,
    handle: Arc<Mutex<Option<Fixture>>>,
}
#[component]
fn Scene(props: &SceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let enabled = hooks.use_state(|| true);
    let text = hooks.use_state(|| TextFieldState::new("Selected"));
    let choice = hooks.use_state(|| {
        ComboBoxState::new(
            vec![SelectOption::available(
                OptionKey::new("selected").unwrap(),
                "Selected",
            )],
            Some(OptionKey::new("selected").unwrap()),
        )
        .unwrap()
    });
    let stack = use_surface_stack(&mut hooks, |stack| {
        let control = ComboBoxControl::new(ComboBoxBinding {
            parent: SurfaceContext {
                stack,
                surface: SurfaceId::Screen,
            },
            parent_scroll: None,
            field: target("field"),
            value: choice,
            on_pick: Handler::default(),
        });
        let availability = AvailabilityFn::new(move || {
            if enabled.get() {
                Availability::Available
            } else {
                Availability::Unavailable {
                    reason: "Loading selected source".into(),
                }
            }
        });
        let open = ActionDef {
            id: ActionId::new("open").unwrap(),
            title: "Open".into(),
            toolbar_label: None,
            keys: vec!["F3".parse().unwrap()],
            availability: availability.clone(),
            choice: None,
            placement: Placement::Hidden,
            run: Handler::from(move |()| {
                control.open().unwrap();
            }),
        };
        let field = TargetSpec::new(target("field")).with_availability(availability);
        let field = match props.kind {
            ControlKind::Combo => field.with_activation(open.id.clone()),
            ControlKind::Editor => {
                field.with_input(TextFieldState::input_handler(text, Handler::default()))
            }
        };
        SurfaceModel {
            focus: FocusModel::new(vec![field, TargetSpec::new(target("other"))]).unwrap(),
            actions: ActionRegistry::new(ScopeId::new("disabled-controls").unwrap(), vec![open])
                .unwrap(),
            primary_action: None,
        }
    });
    *props.handle.lock().unwrap() = Some(Fixture {
        stack,
        enabled,
        text,
    });
    let journal = props.journal.clone();
    element!(InputOwner(stack,width:70u16,height:16u16,quit_action:ActionId::new("quit").unwrap(),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(stack.snapshot().top_id(),text.read().text().as_bytes().to_vec())]))) {OverlayHost {View(width:60,flex_direction:FlexDirection::Column) {
        #(match props.kind {ControlKind::Combo=>element!(ComboBox(id:target("field"),value:choice)).into_any(),ControlKind::Editor=>element!(TextField(id:target("field"),value:text)).into_any()})
        Text(content:"Other control")
    }}})
}
#[tokio::test]
async fn painted_disabled_combo_and_editor_keep_visible_focus_and_ignore_input_until_reenabled() {
    for kind in [ControlKind::Combo, ControlKind::Editor] {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for moved in [false, true] {
                let handle = Arc::new(Mutex::new(None));
                let (mut driver, config, journal) = EventDriver::new();
                let mut ui = element!(Scene(journal,kind,handle:handle.clone()));
                let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    driver.initial(&mut frames).await;
                    let mut fixture = handle.lock().unwrap().unwrap();
                    fixture.enabled.set(false);
                    driver.initial(&mut frames).await;
                    assert_eq!(
                        fixture.stack.snapshot().top().focus.current(),
                        Some(&target("field")),
                        "painted disabled control keeps focus"
                    );
                    let grid = driver.frames.last().unwrap();
                    assert_eq!(
                        grid.cell(0, 0).unwrap().style.foreground,
                        Some(Theme::default().foreground(TextRole::Disabled))
                    );
                    assert_eq!(
                        grid.cell(0, 0).unwrap().style.background,
                        Some(Theme::default().background(SurfaceRole::Selected)),
                        "disabled focus remains visible"
                    );
                    assert!(
                        (0..3).any(|row| grid.row_text(row).contains("Loading selected source")),
                        "disabled reason stays painted"
                    );
                    driver
                        .drive_checked(
                            &mut frames,
                            mode,
                            vec![
                                (key(KeyCode::Enter), false),
                                (key(KeyCode::F(3)), false),
                                (key(KeyCode::Char('x')), false),
                                (TerminalEvent::BracketedPaste("ignored".into()), false),
                            ],
                            false,
                            |_| {
                                assert_eq!(
                                    fixture.stack.snapshot().top_id(),
                                    SurfaceId::Screen,
                                    "disabled activation cannot open a dropdown"
                                );
                                assert_eq!(
                                    fixture.text.read().text(),
                                    "Selected",
                                    "disabled editor cannot edit"
                                );
                            },
                        )
                        .await;
                    if moved {
                        driver
                            .drive(&mut frames, mode, vec![(key(KeyCode::Tab), true)], true)
                            .await;
                        assert_eq!(
                            fixture.stack.snapshot().top().focus.current(),
                            Some(&target("other"))
                        );
                    }
                    fixture.enabled.set(true);
                    driver.initial(&mut frames).await;
                    assert_eq!(
                        fixture.stack.snapshot().top().focus.current(),
                        Some(&target(if moved { "other" } else { "field" })),
                        "re-enable preserves the user's focus choice"
                    );
                    if !moved {
                        match kind {
                            ControlKind::Combo => {
                                driver
                                    .drive_checked(
                                        &mut frames,
                                        mode,
                                        vec![(key(KeyCode::Enter), true)],
                                        true,
                                        |_| {
                                            assert_ne!(
                                                fixture.stack.snapshot().top_id(),
                                                SurfaceId::Screen,
                                                "enabled Combo opens through the real control"
                                            )
                                        },
                                    )
                                    .await;
                            }
                            ControlKind::Editor => {
                                driver
                                    .drive_checked(
                                        &mut frames,
                                        mode,
                                        vec![(key(KeyCode::Char('x')), true)],
                                        true,
                                        |_| assert_eq!(fixture.text.read().text(), "Selectedx"),
                                    )
                                    .await;
                            }
                        }
                    }
                })
                .await
                .unwrap();
            }
        }
    }
}
