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
        controls::{Button, DialogFrame, HelpControl, HelpDialog, ScrollHandle, button_target},
        focus_model::{FocusModel, TargetId, TargetSpec, TargetVisibility},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, SurfaceStackHandle, use_surface_stack},
        surface_stack::{
            OverlayId, OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId,
            SurfaceModel,
        },
    };
    use iocraft::prelude::*;
    use std::sync::{Arc, Mutex};
    fn action(name: &str) -> ActionId {
        ActionId::new(name).unwrap()
    }
    fn target(name: &str) -> TargetId {
        TargetId::for_action(&action(name))
    }
    fn scroll_target() -> TargetId {
        TargetId::new("replacement-scroll").unwrap()
    }
    fn key(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
    }
    #[derive(Clone, Copy)]
    enum ReplacementPhase {
        Ready,
        Pending,
        Hidden,
    }
    #[derive(Clone)]
    struct ReplacementBinding {
        help: HelpControl,
        stack: SurfaceStackHandle,
        scroll: ScrollHandle,
        effects: State<usize>,
    }
    #[derive(Clone)]
    struct ReplacementFixture {
        stack: SurfaceStackHandle,
        id: OverlayId,
        binding: ReplacementBinding,
    }
    fn model_for(phase: ReplacementPhase, binding: &ReplacementBinding) -> SurfaceModel {
        let effects = binding.effects;
        let definition =
            |name: &str, title: &'static str, keys: &[&str], placement, run| ActionDef {
                id: action(name),
                title: title.into(),
                toolbar_label: None,
                keys: keys.iter().map(|key| key.parse().unwrap()).collect(),
                availability: AvailabilityFn::default(),
                choice: None,
                placement,
                run,
            };
        let body = definition("body", "Body", &[], Placement::Toolbar, Handler::default());
        let mut credits = definition(
            "credits",
            "Credits",
            &["F8"],
            Placement::Toolbar,
            Handler::from(move |()| {
                let mut effects = effects;
                effects.set(effects.get() + 1);
            }),
        );
        credits.availability = AvailabilityFn::new(move || match phase {
            ReplacementPhase::Ready | ReplacementPhase::Hidden => Availability::Available,
            ReplacementPhase::Pending => Availability::Unavailable {
                reason: "Operation pending".into(),
            },
        });
        if matches!(phase, ReplacementPhase::Hidden) {
            credits.placement = Placement::Hidden;
        }
        let other = definition(
            "other",
            "Other",
            &["F3"],
            Placement::Toolbar,
            Handler::default(),
        );
        let help = definition(
            "help",
            "Help",
            &["F1"],
            Placement::Hidden,
            binding
                .help
                .open_handler(Default::default, Handler::from(|error| panic!("{error}"))),
        );
        let stack = binding.stack;
        let cancel = definition(
            "cancel",
            "Cancel",
            &["Esc"],
            Placement::Toolbar,
            Handler::from(move |()| {
                stack.close_top();
            }),
        );
        let mut targets = [&body, &credits, &other, &cancel]
            .into_iter()
            .map(|definition| {
                let target = button_target(TargetId::for_action(&definition.id), definition);
                if definition.id == action("credits") {
                    target.with_visibility(TargetVisibility::new(move || {
                        !matches!(phase, ReplacementPhase::Hidden)
                    }))
                } else {
                    target
                }
            })
            .collect::<Vec<_>>();
        targets.push(
            TargetSpec::new(scroll_target())
                .pointer_only()
                .with_input(binding.scroll.input_handler()),
        );
        SurfaceModel {
            focus: FocusModel::new(targets).unwrap(),
            actions: ActionRegistry::new(
                ScopeId::new("replacement-dialog").unwrap(),
                vec![body, credits, other, cancel, help],
            )
            .unwrap(),
            primary_action: None,
        }
    }
    #[derive(Props)]
    struct ReplacementBodyProps {
        #[iocraft(required)]
        binding: ReplacementBinding,
    }
    #[component]
    fn ReplacementBody(
        props: &ReplacementBodyProps,
        hooks: Hooks,
    ) -> impl Into<AnyElement<'static>> {
        let context = *hooks.use_context::<SurfaceContext>();
        let binding = props.binding.clone();
        let help_action =
            Some(element!(Button(id:target("help"),action:action("help"))).into_any());
        if let Some(scroll) = binding.help.scroll(context.surface) {
            return element!(DialogFrame(title:"Model replacement".to_owned(),scroll,scroll_target:binding.help.scroll_target(),scroll_focus:Some(binding.help.body_target()),help_action) {HelpDialog(control:binding.help)}).into_any();
        }
        element!(DialogFrame(title:"Model replacement".to_owned(),scroll:binding.scroll.clone(),scroll_target:scroll_target(),help_action) {Text(content:format!("Credits effects {}",binding.effects.get()))}).into_any()
    }
    #[derive(Props)]
    struct ReplacementSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        handle: Arc<Mutex<Option<ReplacementFixture>>>,
    }
    #[component]
    fn ReplacementScene(
        props: &ReplacementSceneProps,
        mut hooks: Hooks,
    ) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let effects = hooks.use_state(|| 0usize);
        let stack = use_surface_stack(&mut hooks, |_| SurfaceModel {
            focus: FocusModel::new(Vec::new()).unwrap(),
            actions: ActionRegistry::new(ScopeId::new("replacement-screen").unwrap(), Vec::new())
                .unwrap(),
            primary_action: None,
        });
        let binding = hooks
            .use_ref(|| ReplacementBinding {
                stack,
                help: HelpControl::new(stack, action("help")),
                scroll: ScrollHandle::new(stack),
                effects,
            })
            .read()
            .clone();
        let mut opened = hooks.use_ref(|| false);
        if !opened.get() {
            let view_binding = binding.clone();
            let id = stack
                .open_overlay(OverlaySpec {
                    kind: OverlayKind::Dialog,
                    model: model_for(ReplacementPhase::Ready, &binding),
                    view: OverlayView::new(move || {
                        element!(ReplacementBody(binding:view_binding.clone())).into_any()
                    }),
                    layout: OverlayLayout::AtCells {
                        left: 4,
                        top: 2,
                        width: 90,
                        height: 20,
                    },
                })
                .unwrap();
            *props.handle.lock().unwrap() = Some(ReplacementFixture { stack, id, binding });
            opened.set(true);
        }
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:120u16,height:30u16,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(stack.snapshot().top_id(),effects.get().to_le_bytes().to_vec())]))) {OverlayHost})
    }
    fn replace_phase(fixture: &ReplacementFixture, phase: ReplacementPhase) {
        let replacement = model_for(phase, &fixture.binding);
        assert!(
            fixture
                .stack
                .update_overlay_if_open(fixture.id, |current| *current = replacement)
        );
        assert_eq!(
            fixture.stack.snapshot().top_id(),
            SurfaceId::Overlay(fixture.id)
        );
    }
    #[tokio::test]
    async fn replacing_the_whole_dialog_model_preserves_displaced_intent_and_explicit_choices() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for phase in [ReplacementPhase::Pending, ReplacementPhase::Hidden] {
                for moved in [false, true] {
                    let handle = Arc::new(Mutex::new(None));
                    let (mut driver, config, journal) = EventDriver::new();
                    let mut ui = element!(ReplacementScene(journal,handle:handle.clone()));
                    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                    tokio::time::timeout(std::time::Duration::from_secs(5),async {
                    driver.initial(&mut frames).await;let fixture=handle.lock().unwrap().clone().unwrap();driver.drive(&mut frames,mode,vec![(key(KeyCode::Tab),true)],true).await;assert_eq!(fixture.stack.snapshot().top().focus.current(),Some(&target("credits")));
                    replace_phase(&fixture,phase);driver.initial(&mut frames).await;assert_eq!(fixture.stack.snapshot().top().focus.current(),Some(&target(if matches!(phase,ReplacementPhase::Hidden){"body"}else{"credits"})));
                    let map=fixture.stack.hit_regions().published().unwrap();assert_eq!(map.targets.iter().any(|region|region.surface==SurfaceId::Overlay(fixture.id)&&region.target==target("credits")),!matches!(phase,ReplacementPhase::Hidden),"layout presence agrees with the drawn control");
                    driver.drive_checked(&mut frames,mode,vec![(key(KeyCode::Enter),false)],false,|_| {
                        assert_eq!(fixture.stack.snapshot().top_id(),SurfaceId::Overlay(fixture.id),"Enter during pending keeps the dialog open");
                        assert_eq!(fixture.binding.effects.get(),0,"Enter never activates pending Credits or Cancel");
                    }).await;
                    if moved {driver.drive(&mut frames,mode,vec![(key(KeyCode::Tab),true)],true).await;assert_eq!(fixture.stack.snapshot().top().focus.current(),Some(&target("other")));}
                    replace_phase(&fixture,ReplacementPhase::Ready);driver.initial(&mut frames).await;assert_eq!(fixture.stack.snapshot().top().focus.current(),Some(&target(if moved {"other"}else{"credits"})),"complete model replacement retains the focus owner's intent");
                    driver.drive_checked(&mut frames,mode,vec![(key(KeyCode::Enter),!moved)],!moved,|_|assert_eq!(fixture.binding.effects.get(),usize::from(!moved),"Enter reaches the retained target after a complete model refresh")).await;
                }).await.unwrap();
                }
            }
        }
    }
    #[tokio::test]
    async fn replacing_a_help_suspended_dialog_model_preserves_return_intent() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for phase in [ReplacementPhase::Pending, ReplacementPhase::Hidden] {
                let handle = Arc::new(Mutex::new(None));
                let (mut driver, config, journal) = EventDriver::new();
                let mut ui = element!(ReplacementScene(journal,handle:handle.clone()));
                let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    driver.initial(&mut frames).await;
                    let fixture = handle.lock().unwrap().clone().unwrap();
                    driver
                        .drive(&mut frames, mode, vec![(key(KeyCode::Tab), true)], true)
                        .await;
                    replace_phase(&fixture, phase);
                    driver.initial(&mut frames).await;
                    assert_eq!(
                        fixture.stack.snapshot().top().focus.current(),
                        Some(&target(if matches!(phase, ReplacementPhase::Hidden) {
                            "body"
                        } else {
                            "credits"
                        }))
                    );
                    driver
                        .drive(&mut frames, mode, vec![(key(KeyCode::F(1)), true)], true)
                        .await;
                    assert!(
                        fixture
                            .binding
                            .help
                            .is_active(SurfaceId::Overlay(fixture.id))
                    );
                    replace_phase(&fixture, ReplacementPhase::Ready);
                    let source = fixture
                        .binding
                        .help
                        .source(SurfaceId::Overlay(fixture.id))
                        .unwrap();
                    let credits = source
                        .definitions()
                        .iter()
                        .find(|definition| definition.id == action("credits"))
                        .unwrap();
                    assert_eq!(
                        credits.availability.get(),
                        Availability::Available,
                        "Help reads the continuing model before awaiting its changed frame"
                    );
                    assert_eq!(credits.placement, Placement::Toolbar);
                    if matches!(phase, ReplacementPhase::Pending) {
                        driver.initial(&mut frames).await;
                    }
                    assert!(
                        fixture
                            .binding
                            .help
                            .is_active(SurfaceId::Overlay(fixture.id)),
                        "refresh keeps the active Help model"
                    );
                    driver
                        .drive(&mut frames, mode, vec![(key(KeyCode::Esc), true)], true)
                        .await;
                    assert!(
                        !fixture
                            .binding
                            .help
                            .is_active(SurfaceId::Overlay(fixture.id))
                    );
                    assert_eq!(
                        fixture.stack.snapshot().top().focus.current(),
                        Some(&target("credits")),
                        "the saved base model retains displaced Credits intent"
                    );
                    driver
                        .drive_checked(
                            &mut frames,
                            mode,
                            vec![(key(KeyCode::Enter), true)],
                            true,
                            |_| assert_eq!(fixture.binding.effects.get(), 1),
                        )
                        .await;
                })
                .await
                .unwrap();
            }
        }
    }
    #[tokio::test]
    async fn no_safe_focus_keeps_enter_inert_and_scope_help_and_escape_working() {
        let handle = Arc::new(Mutex::new(None));
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(ReplacementScene(journal,handle:handle.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let fixture = handle.lock().unwrap().clone().unwrap();
            let snapshot = fixture.stack.snapshot();
            let cancel = snapshot
                .top()
                .actions
                .definitions()
                .iter()
                .find(|definition| definition.id == action("cancel"))
                .unwrap();
            let targets = vec![button_target(target("cancel"), cancel)];
            fixture.stack.update_overlay_if_open(fixture.id, |model| {
                model.focus.replace_targets(targets).unwrap()
            });
            // Removing all safe targets changes routing even if the footer's pixels are identical.
            assert_eq!(fixture.stack.snapshot().top().focus.current(), None);
            driver
                .drive_checked(
                    &mut frames,
                    InputMode::Burst,
                    vec![(key(KeyCode::Enter), false)],
                    false,
                    |_| {
                        assert_eq!(
                            fixture.stack.snapshot().top_id(),
                            SurfaceId::Overlay(fixture.id),
                            "Enter cannot arm Cancel"
                        );
                        assert_eq!(fixture.binding.effects.get(), 0);
                    },
                )
                .await;
            driver
                .drive_checked(
                    &mut frames,
                    InputMode::Burst,
                    vec![(key(KeyCode::F(1)), true)],
                    true,
                    |_| {
                        assert!(
                            fixture
                                .binding
                                .help
                                .is_active(SurfaceId::Overlay(fixture.id)),
                            "scope Help remains available without focus"
                        )
                    },
                )
                .await;
            driver
                .drive(
                    &mut frames,
                    InputMode::Burst,
                    vec![(key(KeyCode::Esc), true)],
                    true,
                )
                .await;
            assert_eq!(fixture.stack.snapshot().top().focus.current(), None);
            driver
                .drive_checked(
                    &mut frames,
                    InputMode::Burst,
                    vec![(key(KeyCode::Esc), true)],
                    true,
                    |_| {
                        assert_eq!(
                            fixture.stack.snapshot().top_id(),
                            SurfaceId::Screen,
                            "explicit Escape still closes the scope"
                        )
                    },
                )
                .await;
        })
        .await
        .unwrap();
    }
}
