#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        actions::{ActionId, ActionRegistry, Availability, AvailabilityFn, ScopeId},
        controls::{FilterField, TextField, TextFieldState},
        focus_model::{FocusModel, TargetId, TargetSpec},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::use_surface_stack,
        surface_stack::{SurfaceId, SurfaceModel},
    };
    use iocraft::prelude::*;
    use unicode_width::UnicodeWidthStr;
    const REASON: &str = "Selected machine does not allow editing this workspace folder.";
    fn field() -> TargetId {
        TargetId::new("disabled-workspace").unwrap()
    }
    #[derive(Props)]
    struct FieldReasonSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        width: u16,
        height: u16,
        filter: bool,
    }
    #[component]
    fn FieldReasonScene(
        props: &FieldReasonSceneProps,
        mut hooks: Hooks,
    ) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let value = hooks.use_state(|| TextFieldState::new("workspace/folder"));
        let stack = use_surface_stack(&mut hooks, |_| SurfaceModel {
            focus: FocusModel::new(vec![
                TargetSpec::new(field())
                    .with_availability(AvailabilityFn::new(|| Availability::Unavailable {
                        reason: REASON.into(),
                    }))
                    .with_input(TextFieldState::input_handler(value, Handler::default())),
                TargetSpec::new(TargetId::new("other").unwrap()),
            ])
            .unwrap(),
            actions: ActionRegistry::new(ScopeId::new("field-reason").unwrap(), Vec::new())
                .unwrap(),
            primary_action: None,
        });
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:props.width,height:props.height,quit_action:ActionId::new("quit").unwrap(),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(SurfaceId::Screen,value.read().text().as_bytes().to_vec())]))) {OverlayHost {
            View(width:props.width.saturating_sub(4),flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
                Text(content:"Workspace folder",wrap:TextWrap::NoWrap)
                #(if props.filter {element!(FilterField(id:field(),value)).into_any()}else{element!(TextField(id:field(),value)).into_any()})
            }
        }})
    }
    #[tokio::test]
    async fn disabled_field_keeps_its_label_and_value_above_a_complete_muted_wrapped_reason() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for filter in [false, true] {
                for (width, height) in [
                    (120u16, 36u16),
                    (76, 30),
                    (48, 24),
                    (40, 24),
                    (32, 24),
                    (48, 12),
                ] {
                    let (mut driver, config, journal) = EventDriver::new();
                    let mut ui = element!(FieldReasonScene(journal, width, height, filter));
                    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                    tokio::time::timeout(std::time::Duration::from_secs(5), async {
                        driver.initial(&mut frames).await;
                        let grid = driver.frames.last().unwrap();
                        assert!(
                            grid.row_text(0).contains("Workspace folder"),
                            "full label remains visible"
                        );
                        assert!(
                            grid.row_text(1).contains("workspace/folder"),
                            "full field value keeps its row\n{}",
                            grid.row_text(1)
                        );
                        let rows = (2..grid.height)
                            .map(|row| grid.row_text(row))
                            .collect::<Vec<_>>();
                        assert!(
                            rows.join(" ")
                                .split_whitespace()
                                .collect::<Vec<_>>()
                                .join(" ")
                                .contains(REASON),
                            "complete reason wraps below the editor at {width}x{height}\n{}",
                            rows.join("\n")
                        );
                        let first = grid.row_text(2);
                        let column = first
                            .get(..first.find("Selected").unwrap())
                            .unwrap()
                            .width();
                        let muted = Some(Color::AnsiValue(7));
                        assert_eq!(
                            grid.cell(column, 2).unwrap().style.foreground,
                            muted,
                            "reason is muted"
                        );
                        driver
                            .drive_checked(
                                &mut frames,
                                mode,
                                vec![
                                    (TerminalEvent::BracketedPaste("ignored".into()), false),
                                    (
                                        TerminalEvent::Key(KeyEvent::new(
                                            KeyEventKind::Press,
                                            KeyCode::Char('x'),
                                        )),
                                        false,
                                    ),
                                ],
                                false,
                                |ack| assert_eq!(ack.snapshot.0[0].1, b"workspace/folder"),
                            )
                            .await;
                    })
                    .await
                    .unwrap();
                }
            }
        }
    }
}
