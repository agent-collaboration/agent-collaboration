//! Isolated fake picker. One event owner; children render and never subscribe.
#[path = "probe_support/demo_shell_state.rs"]
mod demo_shell_state;
#[path = "probe_support/demo_shell_surfaces.rs"]
mod demo_shell_surfaces;

use demo_shell_state::DemoShellState;
use demo_shell_surfaces::{OverlaySurface, shell_text};
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
    if state.width < 96 || state.height < 24 {
        return element! {
            View(width: u32::from(state.width), height: u32::from(state.height)) {
                Text(content: "Use at least 96×24 cells for this demo. Ctrl+C quits.")
            }
        }
        .into_any();
    }
    let dimmed = !state.overlays.is_empty();
    let sessions = state.visible_sessions();
    let overlay_count = state.overlays.len();
    element! {
        View(width: u32::from(state.width), height: u32::from(state.height), flex_direction: FlexDirection::Column, overflow: Overflow::Hidden) {
            View(height: 1, flex_shrink: 0.0_f32, padding_left: 2, padding_right: 2, flex_direction: FlexDirection::Row, gap: 1) {
                View(width: 8, flex_shrink: 0.0_f32) { #(Some(shell_text("☰ Menu", dimmed))) }
                View(width: 9, flex_shrink: 0.0_f32) { #(Some(shell_text("[ + New ]", dimmed))) }
                View(width: 13, flex_shrink: 0.0_f32) { #(Some(shell_text("[ / Filter… ]", dimmed))) }
                View(width: 23, flex_shrink: 0.0_f32) { #(Some(shell_text(if state.this_folder { "[ This folder | all ]" } else { "[ this folder | All ]" }, dimmed))) }
                View(flex_grow: 1.0_f32) {}
                #(Some(shell_text(format!("Sessions · {}/6", sessions.len()), dimmed)))
            }
            View(height: 1, flex_shrink: 0.0_f32) {}
            View(flex_direction: FlexDirection::Column, gap: 1, padding_left: 2, padding_right: 2, flex_shrink: 0.0_f32) {
                #((sessions.iter()).map(|session| element! {
                    View(height: 2, flex_shrink: 0.0_f32, flex_direction: FlexDirection::Column,
                        background_color: (Some(session.key) == state.selected_session).then_some(if dimmed { Color::Reset } else { Color::DarkBlue })) {
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
            View(height: 2, padding_left: 2, padding_right: 2, flex_shrink: 0.0_f32) {
                #(Some(shell_text("↑↓ move · Enter open · / filter · Ctrl+N new · F10 menu · F1 help · Ctrl+C quit", dimmed)))
            }
            #((state.overlays.into_iter().enumerate()).map(|(index, overlay)| element! {
                OverlaySurface(overlay, machine: state.selected_machine, sort: state.sort, dimmed: index + 1 < overlay_count)
            }))
        }
    }.into_any()
}

#[derive(Props)]
struct DemoPickerShellProps {
    initial_width: u16,
    initial_height: u16,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
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
