#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        actions::{
            ActionDef, ActionId, ActionRegistry, Availability, AvailabilityFn, Placement, ScopeId,
        },
        controls::{HelpControl, Toolbar, button_target, menu_overlay, toolbar_target},
        focus_model::{FocusModel, TargetId},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::{SurfaceStackHandle, use_surface_stack},
        surface_stack::{OverlayLayout, SurfaceId, SurfaceModel},
    };
    use iocraft::prelude::*;
    use std::sync::{Arc, Mutex};
    use tokio::{sync::oneshot, task::JoinHandle};
    fn action(name: &str) -> ActionId {
        ActionId::new(name).unwrap()
    }
    fn target(name: &str) -> TargetId {
        TargetId::for_action(&action(name))
    }
    fn key(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
    }
    #[derive(Clone, Copy)]
    enum ExplicitFocusMove {
        None,
        Tab,
        Pointer,
    }
    struct OperationChannels {
        entered: oneshot::Sender<()>,
        release: oneshot::Receiver<()>,
        completed: oneshot::Sender<()>,
    }
    struct HeldOperation {
        channels: Mutex<Option<OperationChannels>>,
        entered: Mutex<Option<oneshot::Receiver<()>>>,
        release: Mutex<Option<oneshot::Sender<()>>>,
        completed: Mutex<Option<oneshot::Receiver<()>>>,
        task: Mutex<Option<JoinHandle<()>>>,
    }
    impl HeldOperation {
        fn new() -> Self {
            let (entered_tx, entered_rx) = oneshot::channel();
            let (release_tx, release_rx) = oneshot::channel();
            let (completed_tx, completed_rx) = oneshot::channel();
            Self {
                channels: Mutex::new(Some(OperationChannels {
                    entered: entered_tx,
                    release: release_rx,
                    completed: completed_tx,
                })),
                entered: Mutex::new(Some(entered_rx)),
                release: Mutex::new(Some(release_tx)),
                completed: Mutex::new(Some(completed_rx)),
                task: Mutex::new(None),
            }
        }
        fn start(&self, mut enabled: State<bool>) {
            let channels = self.channels.lock().unwrap().take().unwrap();
            enabled.set(false);
            let task = tokio::spawn(async move {
                let _ = channels.entered.send(());
                if channels.release.await.is_ok() {
                    enabled.set(true);
                    let _ = channels.completed.send(());
                }
            });
            *self.task.lock().unwrap() = Some(task);
        }
        async fn wait_started(&self) {
            let entered = self.entered.lock().unwrap().take().unwrap();
            entered.await.unwrap();
        }
        async fn finish_and_join(&self) {
            self.release
                .lock()
                .unwrap()
                .take()
                .unwrap()
                .send(())
                .unwrap();
            let completed = self.completed.lock().unwrap().take().unwrap();
            completed.await.unwrap();
            let task = self.task.lock().unwrap().take().unwrap();
            task.await.unwrap();
        }
    }
    impl Drop for HeldOperation {
        fn drop(&mut self) {
            if let Some(task) = self
                .task
                .get_mut()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                task.abort();
            }
        }
    }
    #[derive(Clone)]
    struct FocusFixture {
        stack: SurfaceStackHandle,
        help: HelpControl,
        credit_activations: State<usize>,
    }
    #[derive(Props)]
    struct FocusReturnSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        #[iocraft(required)]
        operation: Arc<HeldOperation>,
        handle: Arc<Mutex<Option<FocusFixture>>>,
    }
    fn definition(
        name: &str,
        title: &'static str,
        keys: &[&str],
        placement: Placement,
        run: Handler<()>,
    ) -> ActionDef {
        ActionDef {
            id: action(name),
            title: title.into(),
            toolbar_label: None,
            keys: keys.iter().map(|key| key.parse().unwrap()).collect(),
            availability: AvailabilityFn::default(),
            choice: None,
            placement,
            run,
        }
    }
    #[component]
    fn FocusReturnScene(
        props: &FocusReturnSceneProps,
        mut hooks: Hooks,
    ) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let credits_enabled = hooks.use_state(|| true);
        let credit_activations = hooks.use_state(|| 0usize);
        let operation = props.operation.clone();
        let stack = use_surface_stack(&mut hooks, |stack| {
            let help = HelpControl::new(stack, action("help"));
            *props.handle.lock().unwrap() = Some(FocusFixture {
                stack,
                help: help.clone(),
                credit_activations,
            });
            let body = definition("body", "Body", &[], Placement::Toolbar, Handler::default());
            let mut credits = definition(
                "credits",
                "Credits quota control",
                &["F8"],
                Placement::Toolbar,
                Handler::from(move |()| {
                    let mut credit_activations = credit_activations;
                    credit_activations.set(credit_activations.get() + 1);
                }),
            );
            credits.availability = AvailabilityFn::new(move || {
                if credits_enabled.get() {
                    Availability::Available
                } else {
                    Availability::Unavailable {
                        reason: "Operation pending".into(),
                    }
                }
            });
            let other = definition(
                "other",
                "Other",
                &["F3"],
                Placement::Toolbar,
                Handler::default(),
            );
            let mut begin = definition(
                "begin",
                "Begin held operation",
                &["F9"],
                Placement::Hidden,
                Handler::from(move |()| operation.start(credits_enabled)),
            );
            begin.availability = credits.availability.clone();
            let open_help = definition(
                "help",
                "Help",
                &["F1"],
                Placement::Hidden,
                help.open_handler(Default::default, Handler::from(|error| panic!("{error}"))),
            );
            let menu = definition(
                "menu",
                "Menu",
                &["F10"],
                Placement::Hidden,
                Handler::from(move |()| {
                    let snapshot = stack.snapshot();
                    let registry = &snapshot.surface(SurfaceId::Screen).unwrap().actions;
                    stack
                        .open_overlay(
                            menu_overlay(
                                stack,
                                registry,
                                OverlayLayout::AtCells {
                                    left: 10,
                                    top: 5,
                                    width: 40,
                                    height: 8,
                                },
                            )
                            .unwrap(),
                        )
                        .unwrap();
                }),
            );
            let actions = ActionRegistry::new(
                ScopeId::new("focus-return").unwrap(),
                vec![
                    body.clone(),
                    credits.clone(),
                    other.clone(),
                    begin,
                    open_help,
                    menu,
                ],
            )
            .unwrap();
            let targets = vec![
                button_target(target("body"), &body),
                toolbar_target(&credits, actions.clone(), move || stack.viewport().0),
                toolbar_target(&other, actions.clone(), move || stack.viewport().0),
            ];
            SurfaceModel {
                focus: FocusModel::new(targets).unwrap(),
                actions,
                primary_action: None,
            }
        });
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:120u16,height:30u16,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(stack.snapshot().top_id(),credit_activations.get().to_le_bytes().to_vec())]))) {OverlayHost {View(width:100pct,height:100pct,flex_direction:FlexDirection::Column) {
            Toolbar(available_width:stack.viewport().0)
            Text(content:if credits_enabled.get(){"Operation ready"}else{"Operation pending"})
            Text(content:format!("Credits effects {}",credit_activations.get()))
        }}})
    }
    fn assert_focus(driver: &EventDriver, fixture: &FocusFixture, name: &str) {
        assert_eq!(
            fixture.stack.snapshot().top().focus.current(),
            Some(&target(name)),
            "current target is eligible without a repairing Tab"
        );
        let snapshot = fixture.stack.snapshot();
        assert!(snapshot.top().focus.focused().unwrap().can_focus());
        let map = fixture.stack.hit_regions().published().unwrap();
        let region = map
            .targets
            .iter()
            .find(|region| region.surface == snapshot.top_id() && region.target == target(name))
            .unwrap();
        let grid = driver.frames.last().unwrap();
        assert!(
            region.cells.iter().any(|(column, row)| grid
                .cell(usize::from(*column), usize::from(*row))
                .is_some_and(|cell| cell.style.underline)),
            "eligible focus has a visible shipped underline"
        );
    }
    #[tokio::test]
    async fn toolbar_resize_repairs_focus_before_tab_and_returns_it_when_the_button_fits_again() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let handle = Arc::new(Mutex::new(None));
            let operation = Arc::new(HeldOperation::new());
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(FocusReturnScene(journal,operation,handle:handle.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                let fixture = handle.lock().unwrap().clone().unwrap();
                assert_focus(&driver, &fixture, "body");
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::Tab), true)], true)
                    .await;
                assert_focus(&driver, &fixture, "credits");
                driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![(TerminalEvent::Resize(32, 24), true)],
                        true,
                    )
                    .await;
                assert_focus(&driver, &fixture, "body");
                assert!(
                    !driver
                        .frames
                        .last()
                        .unwrap()
                        .row_text(0)
                        .contains("Credits quota control")
                );
                driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![(TerminalEvent::Resize(120, 30), true)],
                        true,
                    )
                    .await;
                assert_focus(&driver, &fixture, "credits");
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Enter), true)],
                        true,
                        |_| {
                            assert_eq!(
                                fixture.credit_activations.get(),
                                1,
                                "Enter reaches the restored Credits action"
                            )
                        },
                    )
                    .await;
            })
            .await
            .unwrap();
        }
    }
    #[tokio::test]
    async fn held_operation_returns_displaced_focus_unless_the_user_explicitly_moves_it() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for movement in [
                ExplicitFocusMove::None,
                ExplicitFocusMove::Tab,
                ExplicitFocusMove::Pointer,
            ] {
                let move_focus = !matches!(movement, ExplicitFocusMove::None);
                let handle = Arc::new(Mutex::new(None));
                let operation = Arc::new(HeldOperation::new());
                let (mut driver, config, journal) = EventDriver::new();
                let mut ui = element!(FocusReturnScene(journal,operation:operation.clone(),handle:handle.clone()));
                let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    driver.initial(&mut frames).await;
                    let fixture = handle.lock().unwrap().clone().unwrap();
                    driver
                        .drive(&mut frames, mode, vec![(key(KeyCode::Tab), true)], true)
                        .await;
                    assert_focus(&driver, &fixture, "credits");
                    driver
                        .drive(&mut frames, mode, vec![(key(KeyCode::F(9)), true)], true)
                        .await;
                    operation.wait_started().await;
                    assert_focus(&driver, &fixture, "body");
                    assert!(
                        driver
                            .frames
                            .last()
                            .unwrap()
                            .row_text(1)
                            .contains("Operation pending")
                    );
                    if move_focus {
                        let event = if matches!(movement, ExplicitFocusMove::Pointer) {
                            let map = fixture.stack.hit_regions().published().unwrap();
                            let region = map
                                .targets
                                .iter()
                                .find(|region| {
                                    region.surface == SurfaceId::Screen
                                        && region.target == target("other")
                                })
                                .unwrap();
                            let (column, row) = region.cells[0];
                            TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                MouseEventKind::Down(MouseButton::Left),
                                column,
                                row,
                            ))
                        } else {
                            key(KeyCode::Tab)
                        };
                        driver
                            .drive(&mut frames, mode, vec![(event, true)], true)
                            .await;
                        assert_focus(&driver, &fixture, "other");
                    }
                    operation.finish_and_join().await;
                    driver.initial(&mut frames).await;
                    assert!(
                        driver
                            .frames
                            .last()
                            .unwrap()
                            .row_text(1)
                            .contains("Operation ready")
                    );
                    assert_focus(
                        &driver,
                        &fixture,
                        if move_focus { "other" } else { "credits" },
                    );
                    driver
                        .drive_checked(
                            &mut frames,
                            mode,
                            vec![(key(KeyCode::Enter), !move_focus)],
                            !move_focus,
                            |_| {
                                assert_eq!(
                                    fixture.credit_activations.get(),
                                    usize::from(!move_focus),
                                    "Enter respects the restored or explicitly selected target"
                                )
                            },
                        )
                        .await;
                })
                .await
                .unwrap();
            }
        }
    }
    #[tokio::test]
    async fn help_and_menu_suspend_displaced_focus_intent_until_the_original_surface_returns() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for help in [false, true] {
                let handle = Arc::new(Mutex::new(None));
                let operation = Arc::new(HeldOperation::new());
                let (mut driver, config, journal) = EventDriver::new();
                let mut ui = element!(FocusReturnScene(journal,operation:operation.clone(),handle:handle.clone()));
                let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    driver.initial(&mut frames).await;
                    let fixture = handle.lock().unwrap().clone().unwrap();
                    driver
                        .drive(&mut frames, mode, vec![(key(KeyCode::Tab), true)], true)
                        .await;
                    driver
                        .drive(&mut frames, mode, vec![(key(KeyCode::F(9)), true)], true)
                        .await;
                    operation.wait_started().await;
                    assert_focus(&driver, &fixture, "body");
                    driver
                        .drive(
                            &mut frames,
                            mode,
                            vec![(key(KeyCode::F(if help { 1 } else { 10 })), true)],
                            true,
                        )
                        .await;
                    let overlay = fixture.stack.snapshot().top_id();
                    assert_ne!(overlay, SurfaceId::Screen);
                    assert_eq!(fixture.help.is_active(overlay), help);
                    operation.finish_and_join().await;
                    driver.initial(&mut frames).await;
                    assert_eq!(
                        fixture.stack.snapshot().top_id(),
                        overlay,
                        "operation settlement does not steal the active overlay"
                    );
                    assert_eq!(
                        fixture
                            .stack
                            .snapshot()
                            .surface(SurfaceId::Screen)
                            .unwrap()
                            .focus
                            .current(),
                        Some(&target("body")),
                        "covered focus intent is suspended"
                    );
                    driver
                        .drive(&mut frames, mode, vec![(key(KeyCode::Esc), true)], true)
                        .await;
                    assert_eq!(fixture.stack.snapshot().top_id(), SurfaceId::Screen);
                    assert_focus(&driver, &fixture, "credits");
                })
                .await
                .unwrap();
            }
        }
    }
}
