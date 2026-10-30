#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        DialogAxisSize, DialogMargins, DialogPresentation,
        actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
        controls::{
            Button, DialogFrame, HelpContent, HelpControl, HelpDialog, HelpInputConvention,
            ScrollHandle, button_target,
        },
        focus_model::{FocusModel, TargetId, TargetSpec},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, use_surface_stack},
        surface_stack::{OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceModel},
    };
    use iocraft::prelude::*;
    fn action(name: &str) -> ActionId {
        ActionId::new(name).unwrap()
    }
    fn body_target() -> TargetId {
        TargetId::new("restore-body").unwrap()
    }
    #[derive(Props)]
    struct RestorePanelProps {
        #[iocraft(required)]
        help: HelpControl,
        #[iocraft(required)]
        scroll: ScrollHandle,
        additional_rows: u16,
    }
    #[component]
    fn RestorePanel(props: &RestorePanelProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
        let surface = *hooks.use_context::<SurfaceContext>();
        let active = props.help.is_active(surface.surface);
        let scroll = props
            .help
            .scroll(surface.surface)
            .unwrap_or_else(|| props.scroll.clone());
        let scroll_target = if active {
            props.help.scroll_target()
        } else {
            body_target()
        };
        let body = if active {
            element!(HelpDialog(control:props.help.clone())).into_any()
        } else {
            element!(View(flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
                Text(content:"FORM FIRST")
                Text(content:"FORM MIDDLE")
                #((0..props.additional_rows).map(|row|element!(Text(content:format!("FORM ROW {row}")))))
                Text(content:"FORM LAST")
            })
            .into_any()
        };
        element!(DialogFrame(title:if active {"Help".to_owned()}else{"New session".to_owned()},scroll,scroll_target,help_action:Some(element!(Button(id:TargetId::for_action(&action("help")),action:action("help"))).into_any())) {#(Some(body))})
    }
    #[derive(Props)]
    struct RestoreSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        additional_rows: u16,
        maximum_height: u16,
    }
    #[component]
    fn RestoreScene(props: &RestoreSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let stack = use_surface_stack(&mut hooks, |_| SurfaceModel {
            focus: FocusModel::new(Vec::new()).unwrap(),
            actions: ActionRegistry::new(ScopeId::new("restore-screen").unwrap(), Vec::new())
                .unwrap(),
            primary_action: None,
        });
        let mut opened = hooks.use_ref(|| false);
        if !opened.get() {
            let help = HelpControl::new(stack, action("help"));
            let open_help = help.open_handler(
                || HelpContent {
                    context: Vec::new(),
                    conventions: (0..40)
                        .map(|row| HelpInputConvention {
                            key: format!("Key {row}"),
                            description: format!("HELP ROW {row}"),
                        })
                        .collect(),
                },
                Handler::from(|error| panic!("{error}")),
            );
            let scroll = ScrollHandle::new(stack);
            let additional_rows = props.additional_rows;
            let definitions = [
                ("save", "Save", "Ctrl+Enter"),
                ("cancel", "Cancel", "Esc"),
                ("help", "Help", "F1"),
            ]
            .into_iter()
            .map(|(name, title, key)| ActionDef {
                id: action(name),
                title: title.into(),
                toolbar_label: None,
                keys: vec![key.parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: if name == "help" {
                    Placement::Hidden
                } else {
                    Placement::Toolbar
                },
                run: if name == "help" {
                    open_help.clone()
                } else {
                    Handler::default()
                },
            })
            .collect::<Vec<_>>();
            let mut targets =
                vec![TargetSpec::new(body_target()).with_input(scroll.input_handler())];
            targets.extend(
                definitions.iter().map(|definition| {
                    button_target(TargetId::for_action(&definition.id), definition)
                }),
            );
            stack
                .open_overlay(OverlaySpec {
                    kind: OverlayKind::Dialog,
                    model: SurfaceModel {
                        focus: FocusModel::new(targets).unwrap(),
                        actions: ActionRegistry::new(
                            ScopeId::new("restore-dialog").unwrap(),
                            definitions,
                        )
                        .unwrap(),
                        primary_action: Some(action("save")),
                    },
                    view: OverlayView::new(move || {
                        element!(RestorePanel(help:help.clone(),scroll:scroll.clone(),additional_rows)).into_any()
                    }),
                    layout: OverlayLayout::Dialog {
                        presentation: DialogPresentation::Sized {
                            width: DialogAxisSize::cells(48),
                            height: DialogAxisSize::Content { maximum: Some(props.maximum_height) },
                        },
                        margins: DialogMargins {
                            top: 1,
                            right: 2,
                            bottom: 1,
                            left: 2,
                        },
                    },
                })
                .unwrap();
            opened.set(true);
        }
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:120u16,height:36u16,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(Vec::new()))) {OverlayHost})
    }
    #[tokio::test]
    async fn content_dialog_restores_exact_eight_row_grid_after_twenty_six_row_help_body() {
        assert_body_restore(0, 26, 8).await;
    }
    #[tokio::test]
    async fn content_dialog_restores_without_retaining_help_padding_near_the_padding_threshold() {
        for (additional_rows, maximum_height, original_height) in
            [(6, 26, 14), (7, 26, 15), (20, 16, 16)]
        {
            assert_body_restore(additional_rows, maximum_height, original_height).await;
        }
    }
    async fn assert_body_restore(
        additional_rows: u16,
        maximum_height: u16,
        original_height: usize,
    ) {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(RestoreScene(journal, additional_rows, maximum_height));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                let original = driver.frames.last().unwrap().clone();
                assert_eq!(
                    original.round_frame_text().lines().count(),
                    original_height,
                    "initial form is content-sized"
                );
                for _ in 0..2 {
                    driver
                        .drive(
                            &mut frames,
                            mode,
                            vec![(
                                TerminalEvent::Key(KeyEvent::new(
                                    KeyEventKind::Press,
                                    KeyCode::F(1),
                                )),
                                true,
                            )],
                            true,
                        )
                        .await;
                    let help = driver.frames.last().unwrap().round_frame_text();
                    assert_eq!(
                        help.lines().count(),
                        usize::from(maximum_height),
                        "Help grows to its capped height\n{help}"
                    );
                    driver
                        .drive(
                            &mut frames,
                            mode,
                            vec![(
                                TerminalEvent::Key(KeyEvent::new(
                                    KeyEventKind::Press,
                                    KeyCode::End,
                                )),
                                true,
                            )],
                            true,
                        )
                        .await;
                    assert!(
                        driver
                            .frames
                            .last()
                            .unwrap()
                            .round_frame_text()
                            .contains("HELP ROW 39"),
                        "the real Help body scrolls"
                    );
                    driver
                        .drive(
                            &mut frames,
                            mode,
                            vec![(
                                TerminalEvent::Key(KeyEvent::new(
                                    KeyEventKind::Press,
                                    KeyCode::Esc,
                                )),
                                true,
                            )],
                            true,
                        )
                        .await;
                    let restored = driver.frames.last().unwrap();
                    assert_eq!(
                        restored.round_frame_text().lines().count(),
                        original_height,
                        "restoration must not retain the Help body's outer height\n{}",
                        restored.round_frame_text()
                    );
                    assert_eq!(
                        restored, &original,
                        "body swap restores the exact original grid and styles"
                    );
                }
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
}
