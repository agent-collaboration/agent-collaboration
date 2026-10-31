#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
        controls::{Button, HelpControl, button_target},
        focus_model::{FocusModel, TargetId},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::use_surface_stack,
        surface_stack::SurfaceModel,
    };
    use iocraft::prelude::*;
    fn action(name: &str) -> ActionId {
        ActionId::new(name).unwrap()
    }
    fn target(name: &str) -> TargetId {
        TargetId::new(name).unwrap()
    }
    fn key(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
    }
    #[derive(Props)]
    struct HintSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        width: u16,
        height: u16,
    }
    #[component]
    fn HintScene(props: &HintSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let calls = hooks.use_state(|| 0usize);
        let retained_help = hooks.use_ref(|| None::<HelpControl>);
        let stack = use_surface_stack(&mut hooks, |stack| {
            let help = HelpControl::new(stack, action("help"));
            let mut retained_help = retained_help;
            retained_help.set(Some(help.clone()));
            let previous = ActionDef {
                id: action("previous"),
                title: "Previous".into(),
                toolbar_label: None,
                keys: vec!["Left".parse().unwrap(), "Ctrl+P".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Toolbar,
                run: Handler::from(move |()| {
                    let mut calls = calls;
                    calls.set(calls.get() + 1);
                }),
            };
            let help_action = ActionDef {
                id: action("help"),
                title: "Help".into(),
                toolbar_label: None,
                keys: vec!["F1".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Hidden,
                run: help.open_handler(Default::default, Handler::from(|error| panic!("{error}"))),
            };
            SurfaceModel {
                focus: FocusModel::new(vec![
                    button_target(target("hidden-hint"), &previous),
                    button_target(target("default-hint"), &previous),
                ])
                .unwrap(),
                actions: ActionRegistry::new(
                    ScopeId::new("button-hint").unwrap(),
                    vec![previous, help_action],
                )
                .unwrap(),
                primary_action: None,
            }
        });
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:props.width,height:props.height,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(stack.snapshot().top_id(),calls.get().to_le_bytes().to_vec())]))) {OverlayHost {View(flex_direction:FlexDirection::Column) {
            Button(id:target("hidden-hint"),action:action("previous"),show_key:false)
            Button(id:target("default-hint"),action:action("previous"))
            Text(content:format!("Calls {}",calls.get()))
        }}})
    }
    #[tokio::test]
    async fn hiding_one_button_hint_preserves_default_hints_every_binding_and_help_aliases() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for (width, height) in [(120u16, 36u16), (48, 12), (32, 24)] {
                let (mut driver, config, journal) = EventDriver::new();
                let mut ui = element!(HintScene(journal, width, height));
                let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    driver.initial(&mut frames).await;
                    let grid = driver.frames.last().unwrap();
                    assert!(
                        grid.row_text(0).contains("Previous") && !grid.row_text(0).contains("Left"),
                        "only the printed hint is hidden\n{}",
                        grid.row_text(0)
                    );
                    assert!(
                        grid.row_text(1).contains("Previous Left"),
                        "default still prints primary key"
                    );
                    driver
                        .drive_checked(
                            &mut frames,
                            mode,
                            vec![(key(KeyCode::Left), true)],
                            true,
                            |ack| {
                                assert_eq!(
                                    ack.snapshot.0[0].1,
                                    1usize.to_le_bytes(),
                                    "hidden hint retains the real binding"
                                )
                            },
                        )
                        .await;
                    let mut alias = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('p'));
                    alias.modifiers = KeyModifiers::CONTROL;
                    driver
                        .drive_checked(
                            &mut frames,
                            mode,
                            vec![(TerminalEvent::Key(alias), true)],
                            true,
                            |ack| {
                                assert_eq!(
                                    ack.snapshot.0[0].1,
                                    2usize.to_le_bytes(),
                                    "all aliases stay active"
                                )
                            },
                        )
                        .await;
                    driver
                        .drive(&mut frames, mode, vec![(key(KeyCode::F(1)), true)], true)
                        .await;
                    let help = driver.frames.last().unwrap().round_frame_text();
                    assert!(
                        help.contains("Previous")
                            && help.contains("Left")
                            && help.contains("Ctrl+P"),
                        "Help retains every binding\n{help}"
                    );
                })
                .await
                .unwrap();
            }
        }
    }
}
