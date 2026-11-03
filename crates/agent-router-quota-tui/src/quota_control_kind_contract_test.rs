//! R15 uses real quota instances and literal typed outcomes, never copied controls.
#[path = "quota_control_kind_fixture_test.rs"]
mod fixture;
#[path = "quota_option_query_contract_test.rs"]
mod option_queries;
#[path = "quota_reset_button_contract_test.rs"]
mod reset_buttons;
#[path = "quota_switch_availability_contract_test.rs"]
mod switch_availability;
use crate::quota_test_support::{
    control_kind_contract::{
        ControlInput, ControlKind, ControlKindContract, ControlKindDeclaration, ControlPaint,
        ControlStep, ScreenControlContracts, assert_control_kind_contract,
        assert_control_kind_matrix,
    },
    event_driver::SurfaceSnapshot,
    frame_grid::FrameStyle,
    grids::STANDARD_GRIDS,
};
use agent_collaboration_tui::{
    actions::ActionId,
    surfaces::{focus_model::TargetId, surface_stack::SurfaceId},
};
use iocraft::prelude::*;
use serde_json::{Value, json};

fn key(code: KeyCode) -> ControlInput {
    ControlInput::Event(TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code)))
}
fn modified(code: KeyCode, modifiers: KeyModifiers) -> ControlInput {
    let mut event = KeyEvent::new(KeyEventKind::Press, code);
    event.modifiers = modifiers;
    ControlInput::Event(TerminalEvent::Key(event))
}
fn target(name: &str) -> TargetId {
    TargetId::for_action(&ActionId::new(format!("quota:{name}")).unwrap())
}
fn pointer(target: TargetId, kind: MouseEventKind) -> ControlInput {
    ControlInput::Pointer { target, kind }
}
fn click(target: TargetId) -> ControlInput {
    pointer(target, MouseEventKind::Down(MouseButton::Left))
}
fn hover(target: TargetId) -> ControlInput {
    pointer(target, MouseEventKind::Moved)
}
fn contains(text: &str) -> ControlPaint {
    ControlPaint::Contains(text.to_owned())
}
fn paint_style(foreground: Color, background: Color, bold: bool, underline: bool) -> FrameStyle {
    FrameStyle {
        foreground: Some(foreground),
        background: Some(background),
        bold,
        underline,
        ..Default::default()
    }
}
fn styled(
    text: &str,
    foreground: Color,
    background: Color,
    bold: bool,
    underline: bool,
) -> ControlPaint {
    ControlPaint::Styled {
        text: text.to_owned(),
        style: paint_style(foreground, background, bold, underline),
    }
}
fn styled_in(
    target: TargetId,
    text: &str,
    foreground: Color,
    background: Color,
    bold: bool,
    underline: bool,
) -> ControlPaint {
    ControlPaint::StyledInTarget {
        target,
        text: text.to_owned(),
        style: paint_style(foreground, background, bold, underline),
    }
}
fn selected_surface() -> Color {
    Color::Rgb {
        r: 52,
        g: 59,
        b: 70,
    }
}
fn prominent() -> Color {
    Color::Rgb {
        r: 237,
        g: 240,
        b: 245,
    }
}
fn base() -> Color {
    Color::Rgb {
        r: 39,
        g: 44,
        b: 52,
    }
}
fn raised() -> Color {
    Color::Rgb {
        r: 48,
        g: 54,
        b: 65,
    }
}
fn primary() -> Color {
    Color::Rgb {
        r: 156,
        g: 188,
        b: 251,
    }
}
fn disabled() -> Color {
    Color::Rgb {
        r: 166,
        g: 175,
        b: 189,
    }
}
fn snapshot(value: Value) -> SurfaceSnapshot {
    SurfaceSnapshot(vec![(
        SurfaceId::Screen,
        serde_json::to_vec(&value).unwrap(),
    )])
}
fn step(
    input: ControlInput,
    redraw: bool,
    state: Value,
    handlers: &[&str],
    paint: Vec<ControlPaint>,
) -> ControlStep {
    ControlStep {
        input,
        redraw,
        expected_snapshot: snapshot(state),
        expected_handlers: handlers.iter().map(|value| (*value).to_owned()).collect(),
        paint,
    }
}
fn contract(
    kind: ControlKind,
    viewport: (u16, u16),
    steps: Vec<ControlStep>,
) -> ControlKindContract {
    ControlKindContract {
        screen: "quota",
        kind,
        viewport,
        initial_paint: vec![contains("Menu F10"), contains("alpha")],
        steps,
    }
}
fn button_state(open: bool) -> Value {
    json!({"dialog":open.then_some("options"),"choice":"No","yesReason":open.then_some("Reset is unavailable")})
}
fn combo_state(
    provider: &str,
    draft: Option<&str>,
    active: Option<(&str, &str, Option<&str>, usize)>,
    credit_phase: Option<&str>,
) -> Value {
    json!({"provider":provider,"disabled":"shown","group":"none","credit":"disallow","dropdown":active.map(|(field,_,_,_)|target(field).as_str().to_owned()),"activeQuery":active.map(|(_,query,_,_)|query),"activeCursor":active.map(|(_,_,_,cursor)|cursor),"activeHighlight":active.and_then(|(_,_,highlight,_)|highlight),"draft":draft,"committed":"All","creditPhase":credit_phase,"notice":null})
}

fn combo_contract(viewport: (u16, u16)) -> ControlKindContract {
    contract(
        ControlKind::ComboBox,
        viewport,
        vec![
            step(
                key(KeyCode::F(3)),
                true,
                combo_state("all", Some("All"), None, None),
                &[],
                vec![
                    contains("Account filters"),
                    contains("Account provider"),
                    styled_in(
                        target("provider-field"),
                        "All",
                        Color::AnsiValue(11),
                        selected_surface(),
                        false,
                        false,
                    ),
                ],
            ),
            step(
                key(KeyCode::Enter),
                true,
                combo_state(
                    "all",
                    Some("All"),
                    Some(("provider-field", "", Some("all"), 0)),
                    None,
                ),
                &[],
                vec![
                    contains("Filter:"),
                    contains("OpenAI"),
                    contains("● All"),
                    styled_in(
                        target("provider-field").child("option:openai"),
                        "OpenAI",
                        Color::AnsiValue(15),
                        raised(),
                        false,
                        false,
                    ),
                ],
            ),
            step(
                key(KeyCode::Down),
                true,
                combo_state(
                    "all",
                    Some("All"),
                    Some(("provider-field", "", Some("openai"), 0)),
                    None,
                ),
                &[],
                vec![contains("OpenAI")],
            ),
            step(
                key(KeyCode::Down),
                true,
                combo_state(
                    "all",
                    Some("All"),
                    Some(("provider-field", "", Some("claude"), 0)),
                    None,
                ),
                &[],
                vec![contains("Claude")],
            ),
            step(
                key(KeyCode::Enter),
                true,
                combo_state("claude", Some("Claude"), None, None),
                &[],
                vec![contains("Claude")],
            ),
            step(
                click(target("provider-field")),
                true,
                combo_state(
                    "claude",
                    Some("Claude"),
                    Some(("provider-field", "", Some("claude"), 0)),
                    None,
                ),
                &[],
                vec![contains("Claude")],
            ),
            step(
                hover(target("provider-field").child("option:openai")),
                true,
                combo_state(
                    "claude",
                    Some("Claude"),
                    Some(("provider-field", "", Some("openai"), 0)),
                    None,
                ),
                &[],
                vec![
                    contains("OpenAI"),
                    styled_in(
                        target("provider-field").child("option:openai"),
                        "OpenAI",
                        Color::AnsiValue(11),
                        selected_surface(),
                        false,
                        false,
                    ),
                ],
            ),
            step(
                click(target("provider-field").child("option:openai")),
                true,
                combo_state("openai", Some("OpenAI"), None, None),
                &[],
                vec![contains("OpenAI")],
            ),
            step(
                key(KeyCode::Esc),
                true,
                combo_state("openai", None, None, None),
                &[],
                vec![contains("Menu F10"), contains("alpha")],
            ),
        ],
    )
}

fn switch_contract(viewport: (u16, u16)) -> ControlKindContract {
    contract(
        ControlKind::SegmentedSwitch,
        viewport,
        vec![
            step(
                modified(KeyCode::Char('r'), KeyModifiers::CONTROL),
                true,
                json!({"tab":"Resets","pending":null,"focused":target("domain-body").as_str()}),
                &[],
                vec![
                    contains("Resets"),
                    styled_in(
                        target("credits-tab"),
                        "Credits",
                        Color::AnsiValue(15),
                        base(),
                        false,
                        false,
                    ),
                ],
            ),
            step(
                modified(KeyCode::Tab, KeyModifiers::SHIFT),
                true,
                json!({"tab":"Resets","pending":null,"focused":target("credits-tab").as_str()}),
                &[],
                vec![styled_in(
                    target("credits-tab"),
                    "Credits",
                    Color::AnsiValue(11),
                    base(),
                    true,
                    true,
                )],
            ),
            step(
                key(KeyCode::Enter),
                true,
                json!({"tab":"Resets","pending":"Credits","focused":target("domain-body").as_str()}),
                &[],
                vec![contains("● Credits"), contains("Credit balance")],
            ),
            step(
                hover(target("resets-tab")),
                true,
                json!({"tab":"Credits","pending":null,"focused":target("credits-tab").as_str()}),
                &[],
                vec![styled_in(
                    target("resets-tab"),
                    "Resets",
                    Color::AnsiValue(15),
                    raised(),
                    false,
                    false,
                )],
            ),
            step(
                click(target("resets-tab")),
                true,
                json!({"tab":"Credits","pending":"Resets","focused":target("domain-body").as_str()}),
                &[],
                vec![
                    contains("● Resets"),
                    styled_in(
                        target("resets-tab"),
                        "Resets",
                        Color::AnsiValue(11),
                        raised(),
                        true,
                        true,
                    ),
                ],
            ),
            step(
                hover(target("credits-tab")),
                true,
                json!({"tab":"Resets","pending":null,"focused":target("resets-tab").as_str()}),
                &[],
                vec![
                    contains("● Resets"),
                    styled_in(
                        target("resets-tab"),
                        "Resets",
                        Color::AnsiValue(11),
                        base(),
                        true,
                        true,
                    ),
                ],
            ),
        ],
    )
}

fn list_contract(viewport: (u16, u16)) -> ControlKindContract {
    let state = |account: &str| json!({"selected":account});
    let alpha = target("accounts").child("row:test-alpha");
    contract(
        ControlKind::ScrollingList,
        viewport,
        vec![
            step(
                key(KeyCode::Down),
                true,
                state("test-beta"),
                &[],
                vec![contains("❯ beta")],
            ),
            step(
                key(KeyCode::Home),
                viewport.1 == 12,
                state("test-beta"),
                &[],
                vec![
                    contains("alpha"),
                    styled_in(alpha.clone(), "alpha", prominent(), base(), true, false),
                ],
            ),
            step(
                hover(alpha.clone()),
                false,
                state("test-beta"),
                &[],
                vec![
                    contains("alpha"),
                    styled_in(alpha.clone(), "alpha", prominent(), base(), true, false),
                ],
            ),
            step(
                click(alpha.clone()),
                true,
                state("test-alpha"),
                &[],
                vec![
                    contains("❯ alpha"),
                    styled_in(
                        alpha.clone(),
                        "alpha",
                        Color::AnsiValue(11),
                        selected_surface(),
                        true,
                        false,
                    ),
                ],
            ),
            step(
                key(KeyCode::Up),
                false,
                state("test-alpha"),
                &[],
                vec![
                    contains("❯ alpha"),
                    styled_in(
                        alpha.clone(),
                        "alpha",
                        Color::AnsiValue(11),
                        selected_surface(),
                        true,
                        false,
                    ),
                ],
            ),
            step(
                key(KeyCode::Tab),
                true,
                state("test-alpha"),
                &[],
                vec![
                    contains("● alpha"),
                    styled_in(
                        alpha.clone(),
                        "alpha",
                        prominent(),
                        selected_surface(),
                        true,
                        false,
                    ),
                ],
            ),
            step(
                modified(KeyCode::Tab, KeyModifiers::SHIFT),
                true,
                state("test-alpha"),
                &[],
                vec![
                    contains("❯ alpha"),
                    styled_in(
                        alpha,
                        "alpha",
                        Color::AnsiValue(11),
                        selected_surface(),
                        true,
                        false,
                    ),
                ],
            ),
        ],
    )
}

fn form_contract(viewport: (u16, u16)) -> ControlKindContract {
    let state = || json!({"value":"all","draft":"All","committed":"All"});
    contract(
        ControlKind::FormField,
        viewport,
        vec![
            step(
                key(KeyCode::F(3)),
                true,
                state(),
                &[],
                vec![
                    styled("Account provider", primary(), base(), false, false),
                    contains("❯ All"),
                ],
            ),
            step(
                key(KeyCode::Tab),
                true,
                state(),
                &[],
                vec![
                    styled("Account provider", primary(), base(), false, false),
                    contains("❯ Shown"),
                ],
            ),
            step(
                click(target("provider-field")),
                true,
                state(),
                &[],
                vec![contains("Filter:")],
            ),
            step(
                key(KeyCode::Esc),
                true,
                state(),
                &[],
                vec![styled("Account provider", primary(), base(), false, false)],
            ),
        ],
    )
}

#[tokio::test]
async fn quota_actual_control_matrix_covers_all_present_kinds_and_explicit_absences() {
    let summary=assert_control_kind_matrix(vec![ScreenControlContracts {
        screen:"quota",
        kinds:vec![
            ControlKindDeclaration::NotOnScreen {kind:ControlKind::TextField,reason:"Quota floor uses adjustment Buttons; options use ComboBoxes, with no TextField instance."},
            ControlKindDeclaration::NotOnScreen {kind:ControlKind::FilterField,reason:"The ComboBox query is its internal HitRegion and EditorLine; quota mounts no FilterField."},
            ControlKindDeclaration::OnScreen {kind:ControlKind::ComboBox,contracts:STANDARD_GRIDS.into_iter().map(combo_contract).collect()},
            ControlKindDeclaration::OnScreen {kind:ControlKind::SegmentedSwitch,contracts:STANDARD_GRIDS.into_iter().map(switch_contract).collect()},
            ControlKindDeclaration::OnScreen {kind:ControlKind::ScrollingList,contracts:STANDARD_GRIDS.into_iter().map(list_contract).collect()},
            ControlKindDeclaration::OnScreen {kind:ControlKind::FormField,contracts:STANDARD_GRIDS.into_iter().map(form_contract).collect()},
            ControlKindDeclaration::OnScreen {kind:ControlKind::Button,contracts:STANDARD_GRIDS.into_iter().map(reset_buttons::button_contract).collect()},
            ControlKindDeclaration::NotOnScreen {kind:ControlKind::DialogKeyHints,reason:"R12 requires a key-hint line only in picker New session; quota uses actual Button hints."},
        ],
    }],fixture::build).await;
    assert_eq!(
        summary.passed_contracts, 20,
        "only five real kinds at four grids count as runtime passes"
    );
    assert_eq!(
        summary
            .not_on_screen
            .iter()
            .map(|absent| absent.kind)
            .collect::<Vec<_>>(),
        vec![
            ControlKind::TextField,
            ControlKind::FilterField,
            ControlKind::DialogKeyHints
        ]
    );
}
