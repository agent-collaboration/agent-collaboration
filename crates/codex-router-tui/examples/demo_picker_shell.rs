//! Isolated fake picker. One event owner; children render and never subscribe.
#[path = "probe_support/demo_shell_captures.rs"]
mod demo_shell_captures;
#[path = "probe_support/demo_shell_choices.rs"]
mod demo_shell_choices;
#[path = "probe_support/demo_shell_dialog_input.rs"]
mod demo_shell_dialog_input;
#[path = "probe_support/demo_shell_input.rs"]
mod demo_shell_input;
#[path = "probe_support/demo_shell_state.rs"]
mod demo_shell_state;
#[path = "probe_support/demo_shell_surfaces.rs"]
mod demo_shell_surfaces;
#[path = "probe_support/demo_shell_theme.rs"]
mod demo_shell_theme;

use codex_router_tui::UiTextRole;
use demo_shell_state::{DemoShellState, ShellFocus};
use demo_shell_surfaces::{OverlaySurface, shell_role_text, shell_text};
use demo_shell_theme::{BackgroundRole, MENU_ICON};
use iocraft::prelude::*;

#[component]
fn DemoPickerShell(
    props: &DemoPickerShellProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let mut model =
        hooks.use_state(|| DemoShellState::new(props.initial_width, props.initial_height));
    let mut system = hooks.use_context_mut::<SystemContext>();
    // Even Resize goes through this subscriber; use_terminal_size would add another.
    hooks.use_terminal_events(move |event| model.write().handle_event(event));
    let state = model.read().clone();
    if state.quit_requested {
        system.exit();
    }
    element!(DemoPickerFrame(state)).into_any()
}

#[derive(Props)]
struct DemoPickerFrameProps {
    #[iocraft(required)]
    state: DemoShellState,
}

#[component]
fn DemoPickerFrame(props: &DemoPickerFrameProps) -> impl Into<AnyElement<'static>> {
    let state = props.state.clone();
    if state.width < 76 || state.height < 24 {
        return element! {
            View(width: u32::from(state.width), height: u32::from(state.height)) {
                Text(content: "Use at least 76×24 cells for this demo. Ctrl+C quits.")
            }
        }
        .into_any();
    }
    let dimmed = !state.overlays.is_empty();
    let sessions = state.visible_sessions();
    let overlay_count = state.overlays.len();
    element! {
        View(width: u32::from(state.width), height: u32::from(state.height), flex_direction: FlexDirection::Column, overflow: Overflow::Hidden, background_color: BackgroundRole::Base.color(dimmed)) {
            View(height: 1, flex_shrink: 0.0_f32, padding_left: 2, padding_right: 2, flex_direction: FlexDirection::Row, gap: 1) {
                View(width: 10, flex_shrink: 0.0_f32, gap: 1, background_color: (state.focus == ShellFocus::Menu).then_some(BackgroundRole::Focused.color(dimmed))) {
                    #(Some(shell_role_text(format!("{MENU_ICON} Menu"), if state.focus == ShellFocus::Menu { UiTextRole::Selected } else { UiTextRole::Body }, dimmed)))
                    #(Some(shell_role_text("F10", UiTextRole::Secondary, dimmed)))
                }
                View(width: 9, flex_shrink: 0.0_f32, background_color: (state.focus == ShellFocus::New).then_some(BackgroundRole::Focused.color(dimmed))) { #(Some(shell_text("[ + New ]", dimmed))) }
                View(width: 13, flex_shrink: 0.0_f32, overflow: Overflow::Hidden, background_color: (state.focus == ShellFocus::Filter).then_some(BackgroundRole::Focused.color(dimmed))) { #(Some(shell_text(if state.focus == ShellFocus::Filter && !dimmed { format!("[ {}▏ ]", state.filter_query) } else if state.filter_query.is_empty() { "[ / Filter… ]".to_owned() } else { format!("[ {} ]", state.filter_query) }, dimmed))) }
                View(width: 21, flex_shrink: 0.0_f32, background_color: (state.focus == ShellFocus::Scope).then_some(BackgroundRole::Focused.color(dimmed))) { #(Some(shell_text(if state.this_folder { "[ This folder | all ]" } else { "[ this folder | All ]" }, dimmed))) }
                View(flex_grow: 1.0_f32) {}
                #(Some(shell_text(format!("Sessions · {}/6", sessions.len()), dimmed)))
            }
            View(height: 1, flex_shrink: 0.0_f32) {}
            View(flex_direction: FlexDirection::Column, gap: 1, padding_left: 2, padding_right: 2, flex_shrink: 0.0_f32) {
                #((sessions.iter()).map(|session| element! {
                    View(height: 2, flex_shrink: 0.0_f32, flex_direction: FlexDirection::Column,
                        background_color: (Some(session.key) == state.selected_session).then_some(BackgroundRole::Focused.color(dimmed))) {
                        View(height: 1, flex_direction: FlexDirection::Row, gap: 1) {
                            View(width: 1) { #(Some(shell_text(if Some(session.key) == state.selected_session { "❯" } else { "" }, dimmed))) }
                            #(Some(shell_text(session.title, dimmed)))
                        }
                        View(height: 1, flex_direction: FlexDirection::Row) {
                            View(width: 14, flex_shrink: 0.0_f32) { #(Some(shell_text("Codex · main", dimmed))) }
                            #(Some(shell_text("📂", dimmed)))
                            #(Some(shell_text(session.folder, dimmed)))
                        }
                    }
                }))
                #(sessions.is_empty().then(|| shell_text("No matching sessions", dimmed)))
            }
            View(flex_grow: 1.0_f32) {}
            View(height: 1, padding_left: 2, flex_shrink: 0.0_f32) { #(Some(shell_text(format!("Last action: {}", state.last_action), dimmed))) }
            #((state.overlays.into_iter().enumerate()).map(|(index, overlay)| element! {
                OverlaySurface(overlay, machine: state.selected_machine, client: state.selected_client, directory: state.selected_directory, sort: state.sort, dimmed: index + 1 < overlay_count)
            }))
        }
    }.into_any()
}

#[derive(Props)]
struct DemoPickerShellProps {
    initial_width: u16,
    initial_height: u16,
}

fn render_demo_canvas(state: DemoShellState) -> Canvas {
    let width = state.width;
    element!(DemoPickerFrame(state)).render(Some(usize::from(width)))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--capture") {
        return demo_shell_captures::capture_owner_frames();
    }
    // The pinned Hooks size accessor is private. stty samples dimensions once,
    // without registering another terminal-event subscriber or reading keys.
    let output = std::process::Command::new("/bin/stty")
        .arg("size")
        .stdin(std::process::Stdio::inherit())
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other("stty size requires a terminal"));
    }
    let dimensions = String::from_utf8_lossy(&output.stdout);
    let mut dimensions = dimensions.split_whitespace();
    let parse_dimension = |value: Option<&str>| -> std::io::Result<u16> {
        value
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| std::io::Error::other("invalid terminal dimensions"))
    };
    let initial_height = parse_dimension(dimensions.next())?;
    let initial_width = parse_dimension(dimensions.next())?;
    element!(DemoPickerShell(initial_width, initial_height))
        .render_loop()
        .fullscreen()
        .enable_bracketed_paste()
        .ignore_ctrl_c()
        .await
}
