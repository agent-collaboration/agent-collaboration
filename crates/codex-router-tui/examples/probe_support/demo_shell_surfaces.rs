use super::demo_shell_state::{
    ClientChoice, DemoOverlay, DialogChoice, DialogFocus, DirectoryChoice, MachineChoice,
    SessionSort, overlay_rectangle,
};
use iocraft::prelude::*;

pub fn shell_text(content: impl Into<String>, dimmed: bool) -> AnyElement<'static> {
    element!(Text(content: content.into(), color: if dimmed { Color::DarkGrey } else { Color::White }, weight: if dimmed { Weight::Light } else { Weight::Normal })).into_any()
}

#[derive(Props)]
struct MenuRowProps {
    title: String,
    shortcut: String,
    checked: bool,
    selected: bool,
    dimmed: bool,
}
#[component]
fn MenuRow(props: &MenuRowProps) -> impl Into<AnyElement<'static>> {
    element! {
        View(height: 1, flex_direction: FlexDirection::Row, padding_left: 1, padding_right: 1, gap: 1,
            background_color: props.selected.then_some(if props.dimmed { Color::DarkGrey } else { Color::DarkBlue })) {
            View(width: 1) { #(Some(shell_text(if props.checked { "✓" } else { "" }, props.dimmed))) }
            #(Some(shell_text(props.title.clone(), props.dimmed)))
            View(flex_grow: 1.0_f32) {}
            #(Some(shell_text(props.shortcut.clone(), props.dimmed)))
        }
    }
}

#[derive(Props)]
pub struct OverlaySurfaceProps {
    #[iocraft(required)]
    pub overlay: DemoOverlay,
    #[iocraft(required)]
    pub machine: MachineChoice,
    #[iocraft(required)]
    pub sort: SessionSort,
    #[iocraft(required)]
    pub client: ClientChoice,
    #[iocraft(required)]
    pub directory: DirectoryChoice,
    pub dimmed: bool,
}
#[component]
pub fn OverlaySurface(props: &OverlaySurfaceProps) -> impl Into<AnyElement<'static>> {
    let rect = overlay_rectangle(&props.overlay);
    let dimmed = props.dimmed;
    let contents: AnyElement<'static> = match &props.overlay {
        DemoOverlay::Menu { selected } => element! {
            View(flex_direction: FlexDirection::Column) {
                #(Some(shell_text("Menu", dimmed)))
                MenuRow(title: "New session", shortcut: "Ctrl+N", selected: *selected == 0, dimmed)
                MenuRow(title: "Filter sessions", shortcut: "/", selected: *selected == 1, dimmed)
                MenuRow(title: "Sort: Updated", checked: props.sort == SessionSort::Updated, selected: *selected == 2, dimmed)
                MenuRow(title: "Sort: Name", checked: props.sort == SessionSort::Name, selected: *selected == 3, dimmed)
                MenuRow(title: "Help", shortcut: "F1", selected: *selected == 4, dimmed)
                MenuRow(title: "Close menu", shortcut: "Esc", selected: *selected == 5, dimmed)
            }
        }.into_any(),
        DemoOverlay::NewSession { focus } => element! {
            View(flex_direction: FlexDirection::Column, padding_left: 2, padding_right: 2) {
                #(Some(shell_text("New session", dimmed)))
                #(Some(shell_text("Choose client, machine and directory.", dimmed)))
                DialogFieldRow(label: "Client", value: DialogChoice::Client(props.client).label(), focused: *focus == DialogFocus::Client, dimmed)
                DialogFieldRow(label: "Machine", value: props.machine.label(), focused: *focus == DialogFocus::Machine, dimmed)
                DialogFieldRow(label: "Directory", value: DialogChoice::Directory(props.directory).label(), focused: *focus == DialogFocus::Directory, dimmed)
                #(Some(shell_text("Enter opens the focused choice list", dimmed)))
                View(height: 2) {}
                View(height: 1, flex_direction: FlexDirection::Row, gap: 3) {
                    View(width: 22, background_color: (*focus == DialogFocus::Cancel && !dimmed).then_some(Color::DarkBlue)) { #(Some(shell_text("[ Cancel ]", dimmed))) }
                    View(flex_grow: 1.0_f32, background_color: (*focus == DialogFocus::Start && !dimmed).then_some(Color::DarkBlue)) { #(Some(shell_text("[ Start ]", dimmed))) }
                }
                #(Some(shell_text("↑↓ or Tab move · Ctrl+Enter start · Esc cancel", dimmed)))
            }
        }.into_any(),
        DemoOverlay::ChoicePicker { query, selected } => {
            let options = selected.matching(query);
            element! {
                View(flex_direction: FlexDirection::Column, padding_left: 1, padding_right: 1) {
                    #(Some(shell_text(selected.field_label(), dimmed)))
                    #(Some(shell_text(format!("Filter: {query}▏"), dimmed)))
                    #((options.iter()).map(|machine| element! {
                        View(height: 1, background_color: (*machine == *selected).then_some(Color::DarkBlue)) {
                            #(Some(shell_text(machine.label(), dimmed)))
                        }
                    }))
                    #(options.is_empty().then(|| shell_text("No matching machines", dimmed)))
                    View(flex_grow: 1.0_f32) {}
                    #(Some(shell_text("↑↓ move · Enter pick · Esc close", dimmed)))
                }
            }.into_any()
        }
        DemoOverlay::Help => element! {
            View(flex_direction: FlexDirection::Column, padding_left: 2, padding_right: 2, gap: 1) {
                #(Some(shell_text("Picker shell · fake data", dimmed)))
                #(Some(shell_text("F10 menu · Ctrl+N new · / filter", dimmed)))
                #(Some(shell_text("Open Machine, type to filter, Enter to select.", dimmed)))
                #(Some(shell_text("Every lower surface is dimmed and disabled.", dimmed)))
                #(Some(shell_text("Esc closes one layer · Ctrl+C quits", dimmed)))
                #(Some(shell_text("Esc or Enter closes this help", dimmed)))
            }
        }.into_any(),
    };
    element! {
        // The gutter erases the START of an emoji crossing the panel's left edge.
        // It is a separate iocraft sibling, painted before the panel, not string padding.
        View(position: Position::Absolute, left: i32::from(rect.left.saturating_sub(1)), top: i32::from(rect.top),
            width: u32::from(rect.width + 1), height: u32::from(rect.height), flex_direction: FlexDirection::Row) {
            View(width: 1, height: u32::from(rect.height), flex_shrink: 0.0_f32, background_color: Color::Reset) {}
            View(width: u32::from(rect.width), height: u32::from(rect.height), flex_shrink: 0.0_f32,
                flex_direction: FlexDirection::Column,
                background_color: Color::Reset, border_style: BorderStyle::Single,
                border_color: if dimmed { Color::DarkGrey } else { Color::Cyan }) {
                #(Some(contents))
            }
        }
    }
}

#[derive(Props)]
struct DialogFieldRowProps {
    label: String,
    value: String,
    focused: bool,
    dimmed: bool,
}
#[component]
fn DialogFieldRow(props: &DialogFieldRowProps) -> impl Into<AnyElement<'static>> {
    element! {
        View(height: 1, flex_direction: FlexDirection::Row, background_color: (props.focused && !props.dimmed).then_some(Color::DarkBlue)) {
            View(width: 12, flex_shrink: 0.0_f32) { #(Some(shell_text(props.label.clone(), props.dimmed))) }
            #(Some(shell_text(format!("{} ▾", props.value), props.dimmed)))
        }
    }
}
