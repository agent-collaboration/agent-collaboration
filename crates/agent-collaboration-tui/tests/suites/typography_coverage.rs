#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        frame_grid::FrameGrid,
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        UiColumnHeading, UiHeading, UiHeadingLevel, UiText, UiTextRole,
        actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
        focus_model::FocusModel,
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::use_surface_stack,
        surface_stack::{
            OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId, SurfaceModel,
        },
    };
    use iocraft::prelude::*;

    #[test]
    fn consumer_custom_colour_uses_the_shared_covered_foreground_transform() {
        use agent_collaboration_tui::theme::Theme;
        let custom = Color::Rgb {
            r: 19,
            g: 83,
            b: 211,
        };
        assert_eq!(Theme::default().foreground_color(custom), custom);
        assert_eq!(
            Theme::covered().foreground_color(custom),
            Color::Rgb {
                r: 142,
                g: 150,
                b: 163
            }
        );
    }

    const TEXT_CASES: [(UiTextRole, &str, Color); 10] = [
        (UiTextRole::Body, "Body", Color::White),
        (
            UiTextRole::Prominent,
            "Prominent",
            Color::Rgb {
                r: 237,
                g: 240,
                b: 245,
            },
        ),
        (
            UiTextRole::Label,
            "Label",
            Color::Rgb {
                r: 156,
                g: 188,
                b: 251,
            },
        ),
        (
            UiTextRole::Rule,
            "Rule",
            Color::Rgb {
                r: 119,
                g: 200,
                b: 187,
            },
        ),
        (UiTextRole::Secondary, "Secondary", Color::Grey),
        (
            UiTextRole::Disabled,
            "Disabled",
            Color::Rgb {
                r: 166,
                g: 175,
                b: 189,
            },
        ),
        (UiTextRole::Selected, "Selected", Color::Yellow),
        (UiTextRole::Success, "Success", Color::Green),
        (
            UiTextRole::Warning,
            "Warning",
            Color::Rgb {
                r: 255,
                g: 165,
                b: 0,
            },
        ),
        (UiTextRole::Error, "Error", Color::Red),
    ];
    #[component]
    fn TypographySamples() -> impl Into<AnyElement<'static>> {
        element!(View(width:76,height:24,flex_direction:FlexDirection::Column) {
            UiHeading(content:"Surface heading".to_owned(),level:UiHeadingLevel::Surface)
            UiHeading(content:"Section heading".to_owned(),level:UiHeadingLevel::Section)
            UiHeading(content:"Group heading".to_owned(),level:UiHeadingLevel::Group)
            UiHeading(content:"Inactive heading".to_owned(),level:UiHeadingLevel::Surface,inactive:true)
            #(TEXT_CASES.into_iter().map(|(role,label,_)|element!(UiText(content:label.to_owned(),role)).into_any()))
            UiColumnHeading(content:"Column heading".to_owned())
        })
    }
    fn normalized_color(color: Color) -> Color {
        FrameGrid::from_canvas(&element!(Text(content:"x",color)).render(None))
            .unwrap()
            .cell(0, 0)
            .unwrap()
            .style
            .foreground
            .expect("colored reference cell")
    }
    fn assert_palette(frame: &FrameGrid, covered: bool) {
        let mut cases = vec![
            (0, "Surface heading", Color::Cyan, true),
            (
                1,
                "Section heading",
                Color::Rgb {
                    r: 156,
                    g: 188,
                    b: 251,
                },
                true,
            ),
            (2, "Group heading", Color::Grey, true),
            (3, "Inactive heading", Color::Grey, true),
        ];
        cases.extend(
            TEXT_CASES
                .into_iter()
                .enumerate()
                .map(|(index, (role, label, color))| {
                    (index + 4, label, color, role == UiTextRole::Selected)
                }),
        );
        cases.push((
            14,
            "Column heading",
            Color::Rgb {
                r: 237,
                g: 240,
                b: 245,
            },
            true,
        ));
        for (row, label, normal, bold) in cases {
            assert!(
                frame.row_text(row).starts_with(label),
                "row {row}: {}",
                frame.row_text(row)
            );
            let expected = normalized_color(if covered {
                Color::Rgb {
                    r: 142,
                    g: 150,
                    b: 163,
                }
            } else {
                normal
            });
            for column in 0..label.len() {
                let style = &frame.cell(column, row).unwrap().style;
                assert_eq!(
                    style.foreground,
                    Some(expected),
                    "{label} at {column}, covered={covered}"
                );
                assert_eq!(style.bold, bold, "{label}");
            }
        }
        assert_eq!(frame.cell(0, 15).unwrap().text, "─");
        assert_eq!(
            frame.cell(0, 15).unwrap().style.foreground,
            Some(normalized_color(if covered {
                Color::Rgb {
                    r: 142,
                    g: 150,
                    b: 163,
                }
            } else {
                Color::Rgb {
                    r: 119,
                    g: 200,
                    b: 187,
                }
            }))
        );
    }
    #[test]
    fn standalone_typography_keeps_every_shipped_role_and_heading_palette() {
        let canvas = element!(TypographySamples).render(None);
        assert_palette(&FrameGrid::from_canvas(&canvas).unwrap(), false);
    }
    #[derive(Props)]
    struct TypographySceneProps {
        #[iocraft(required)]
        journal: EventJournal,
    }
    #[component]
    fn TypographyScene(
        props: &TypographySceneProps,
        mut hooks: Hooks,
    ) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let stack = use_surface_stack(&mut hooks, |stack| SurfaceModel {
            focus: FocusModel::new(Vec::new()).unwrap(),
            actions: ActionRegistry::new(
                ScopeId::new("typography").unwrap(),
                vec![ActionDef {
                    id: ActionId::new("open-menu").unwrap(),
                    title: "Menu".into(),
                    keys: vec!["F10".parse().unwrap()],
                    availability: AvailabilityFn::default(),
                    choice: None,
                    placement: Placement::Hidden,
                    run: Handler::from(move |()| {
                        stack
                            .open_overlay(OverlaySpec {
                                kind: OverlayKind::Menu,
                                model: SurfaceModel {
                                    focus: FocusModel::new(Vec::new()).unwrap(),
                                    actions: ActionRegistry::new(
                                        ScopeId::new("menu").unwrap(),
                                        Vec::new(),
                                    )
                                    .unwrap(),
                                    primary_action: None,
                                },
                                view: OverlayView::new(|| {
                                    element!(View {Text(content:"Overlay")}).into_any()
                                }),
                                layout: OverlayLayout::AtCells {
                                    left: 54,
                                    top: 1,
                                    width: 16,
                                    height: 4,
                                },
                            })
                            .unwrap();
                    }),
                }],
            )
            .unwrap(),
            primary_action: None,
        });
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:76u16,height:24u16,quit_action:ActionId::new("quit").unwrap(),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(SurfaceId::Screen,vec![])]))) {
            OverlayHost {TypographySamples}
        })
    }
    #[tokio::test]
    async fn covered_typography_dims_every_role_and_restores_the_uncovered_frame() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(TypographyScene(journal));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                let uncovered = driver.frames.last().unwrap().clone();
                assert_palette(&uncovered, false);
                let key = |code| TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code));
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::F(10)), true)], true)
                    .await;
                assert_palette(driver.frames.last().unwrap(), true);
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::Esc), true)], true)
                    .await;
                assert_eq!(driver.frames.last().unwrap(), &uncovered);
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
}
