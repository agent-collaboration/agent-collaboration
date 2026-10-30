#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        frame_grid::FrameGrid,
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        actions::{
            ActionDef, ActionId, ActionRegistry, AvailabilityFn, Checked, Placement, ScopeId,
        },
        controls::{Button, button_target},
        focus_model::{FocusModel, TargetId},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::use_surface_stack,
        surface_stack::{SurfaceId, SurfaceModel},
        theme::{SurfaceRole, Theme},
    };
    use iocraft::prelude::*;
    use unicode_width::UnicodeWidthStr;
    fn action(name: &str) -> ActionId {
        ActionId::new(name).unwrap()
    }
    #[derive(Props)]
    struct ButtonSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        checked: bool,
    }
    #[component]
    fn ButtonScene(props: &ButtonSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let checked = props.checked;
        let stack = use_surface_stack(&mut hooks, |_| {
            let actions = [("alpha", "Alpha action"), ("bravo", "Bravo action")]
                .into_iter()
                .map(|(name, title)| ActionDef {
                    id: action(name),
                    title: title.into(),
                    toolbar_label: None,
                    keys: Vec::new(),
                    availability: AvailabilityFn::default(),
                    choice: Some(Checked::new(move || name == "alpha" && checked)),
                    placement: Placement::Toolbar,
                    run: Handler::default(),
                })
                .collect::<Vec<_>>();
            SurfaceModel {
                focus: FocusModel::new(
                    actions
                        .iter()
                        .map(|action| button_target(TargetId::for_action(&action.id), action))
                        .collect(),
                )
                .unwrap(),
                actions: ActionRegistry::new(ScopeId::new("button-style").unwrap(), actions)
                    .unwrap(),
                primary_action: None,
            }
        });
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:60u16,height:12u16,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(SurfaceId::Screen,Vec::new())]))) {
            OverlayHost {View(flex_direction:FlexDirection::Column) {
                Button(id:TargetId::for_action(&action("alpha")),action:action("alpha"))
                Button(id:TargetId::for_action(&action("bravo")),action:action("bravo"))
            }}
        })
    }
    fn assert_label_style(frame: &FrameGrid, label: &str, focused: bool, checked: bool) {
        let (row, line) = (0..frame.height)
            .map(|row| (row, frame.row_text(row)))
            .find(|(_, line)| line.contains(label))
            .unwrap();
        let byte = line.find(label).unwrap();
        let column = line.get(..byte).unwrap().width();
        for offset in 0..label.len() {
            let style = &frame.cell(column + offset, row).unwrap().style;
            assert_eq!(
                style.foreground,
                Some(Color::AnsiValue(if focused || checked { 11 } else { 15 })),
                "{label}"
            );
            assert_eq!(
                style.bold,
                focused || checked,
                "{label}: focused or checked label must be bold"
            );
            assert_eq!(
                style.underline, focused,
                "{label}: focus remains visible without colour"
            );
            assert_eq!(
                style.background,
                Some(Theme::default().background(SurfaceRole::Base)),
                "{label}: shipped button focus has no extra selection background"
            );
        }
    }
    #[tokio::test]
    async fn focused_and_checked_buttons_keep_shipped_label_styles_without_extra_backgrounds() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for checked in [false, true] {
                let (mut driver, config, journal) = EventDriver::new();
                let mut ui = element!(ButtonScene(journal, checked));
                let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                let scenario = async {
                    driver.initial(&mut frames).await;
                    assert_label_style(
                        driver.frames.last().unwrap(),
                        "Alpha action",
                        true,
                        checked,
                    );
                    assert_label_style(driver.frames.last().unwrap(), "Bravo action", false, false);
                    driver
                        .drive(
                            &mut frames,
                            mode,
                            vec![(
                                TerminalEvent::Key(KeyEvent::new(
                                    KeyEventKind::Press,
                                    KeyCode::Tab,
                                )),
                                true,
                            )],
                            true,
                        )
                        .await;
                    assert_label_style(
                        driver.frames.last().unwrap(),
                        "Alpha action",
                        false,
                        checked,
                    );
                    assert_label_style(driver.frames.last().unwrap(), "Bravo action", true, false);
                };
                tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                    .await
                    .unwrap();
            }
        }
    }
}
