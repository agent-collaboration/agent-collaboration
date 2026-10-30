use super::demo_shell_choices::ChoiceAvailability;
use super::demo_shell_state::{
    ClientChoice, DemoOverlay, DetailsFocus, DialogChoice, DialogFocus, DirectoryChoice,
    FAKE_SESSIONS, MachineChoice, SessionSort, overlay_rectangle,
};
use super::demo_shell_theme::{BackgroundRole, StatusRole};
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
            background_color: props.selected.then_some(BackgroundRole::Focused.color(props.dimmed))) {
            View(width: 1) { #(Some(shell_text(if props.checked { "✓" } else { "" }, props.dimmed))) }
            #(Some(shell_text(props.title.clone(), props.dimmed)))
            View(flex_grow: 1.0_f32) {}
            #(Some(shell_text(props.shortcut.clone(), props.dimmed)))
        }
    }
}
#[derive(Props)]
pub struct GroupPanelProps<'a> {
    pub children: Vec<AnyElement<'a>>,
    pub dimmed: bool,
}
#[component]
pub fn GroupPanel<'a>(props: &mut GroupPanelProps<'a>) -> impl Into<AnyElement<'a>> {
    let children = std::mem::take(&mut props.children);
    element!(View(flex_direction: FlexDirection::Column, padding_left: 1, padding_right: 1,
        background_color: BackgroundRole::Group.color(props.dimmed)) { #(children) })
}
#[derive(Props)]
struct StatusLineProps {
    label: String,
    value: String,
    #[iocraft(required)]
    role: StatusRole,
    dimmed: bool,
}
#[component]
fn StatusLine(props: &StatusLineProps) -> impl Into<AnyElement<'static>> {
    element!(View(height: 1, flex_direction: FlexDirection::Row) {
        View(width: 14, flex_shrink: 0.0_f32) { #(Some(shell_text(props.label.clone(), props.dimmed))) }
        Text(content: props.value.clone(), color: props.role.color(props.dimmed), weight: if props.dimmed { Weight::Light } else { Weight::Normal })
    })
}
#[derive(Props)]
struct ActionButtonProps {
    label: String,
    width: u16,
    focused: bool,
    primary: bool,
    dimmed: bool,
}
#[component]
fn ActionButton(props: &ActionButtonProps) -> impl Into<AnyElement<'static>> {
    let role = if props.primary {
        BackgroundRole::Primary
    } else if props.focused {
        BackgroundRole::Focused
    } else {
        BackgroundRole::Surface
    };
    element!(View(width: u32::from(props.width), height: 1, flex_direction: FlexDirection::Row, gap: 1, background_color: role.color(props.dimmed)) {
        View(width: 1) { #(Some(shell_text(if props.focused { "❯" } else { "" }, props.dimmed))) }
        #(Some(shell_text(props.label.clone(), props.dimmed)))
    })
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
                View(height: 1) {}
                GroupPanel(dimmed) {
                    #(Some(shell_text("Configuration", dimmed)))
                    DialogFieldRow(label: "Client", value: DialogChoice::Client(props.client).label(), focused: *focus == DialogFocus::Client, dropdown: true, dimmed)
                    DialogFieldRow(label: "Machine", value: props.machine.label(), focused: *focus == DialogFocus::Machine, dropdown: true, dimmed)
                    DialogFieldRow(label: "Directory", value: DialogChoice::Directory(props.directory).label(), focused: *focus == DialogFocus::Directory, dropdown: true, dimmed)
                }
                View(height: 1) {}
                GroupPanel(dimmed) {
                    #(Some(shell_text("Availability", dimmed)))
                    StatusLine(label: "Machine", value: "available", role: StatusRole::Available, dimmed)
                    StatusLine(label: "Directory", value: "warning · demo path", role: StatusRole::Warning, dimmed)
                }
                View(height: 1) {}
                View(height: 1, flex_direction: FlexDirection::Row, gap: 3) {
                    ActionButton(label: "Cancel", width: 22u16, focused: *focus == DialogFocus::Cancel, dimmed)
                    ActionButton(label: "Start", width: 21u16, focused: *focus == DialogFocus::Start, primary: true, dimmed)
                }
                #(Some(shell_text("Ctrl+Enter start · Esc close", dimmed)))
            }
        }.into_any(),
        DemoOverlay::ChoicePicker(picker) => {
            let options = picker.visible_options();
            element! {
                View(flex_direction: FlexDirection::Column, padding_left: 1, padding_right: 1) {
                    #(Some(shell_text(picker.anchor.field_label(), dimmed)))
                    #(Some(shell_text(format!("Filter: {}▏", picker.query), dimmed)))
                    #((options.iter()).map(|option| {
                        let unavailable = matches!(option.availability, ChoiceAvailability::Unavailable { .. });
                        element!(View(height: 1, flex_direction: FlexDirection::Row, gap: 1,
                            background_color: (Some(option.key) == picker.highlighted_key()).then_some(BackgroundRole::Focused.color(dimmed))) {
                            View(width: 1) { #(Some(shell_text(if Some(option.key) == picker.highlighted_key() { "❯" } else { "" }, dimmed))) }
                            #(Some(shell_text(option.key.label(), dimmed)))
                            #(unavailable.then(|| element!(Text(content: "unavailable", color: StatusRole::Error.color(dimmed)))))
                        })
                    }))
                    #(options.is_empty().then(|| shell_text("No matching options", dimmed)))
                }
            }.into_any()
        }
        DemoOverlay::Details { session, focus } => {
            let record = FAKE_SESSIONS.iter().find(|record| record.key == *session);
            element! {
                View(flex_direction: FlexDirection::Column, padding_left: 2, padding_right: 2) {
                    #(Some(shell_text("Details", dimmed)))
                    View(height: 1) {}
                    GroupPanel(dimmed) {
                        #(Some(shell_text("Session", dimmed)))
                        #(Some(shell_text(record.map_or("Unavailable session", |record| record.title), dimmed)))
                        #(Some(shell_text(record.map_or("", |record| record.folder), dimmed)))
                    }
                    View(height: 1) {}
                    GroupPanel(dimmed) {
                        #(Some(shell_text("Runtime", dimmed)))
                        DialogFieldRow(label: "Client", value: DialogChoice::Client(props.client).label(), dimmed)
                        DialogFieldRow(label: "Machine", value: props.machine.label(), dimmed)
                    }
                    View(height: 1) {}
                    GroupPanel(dimmed) {
                        #(Some(shell_text("Status", dimmed)))
                        StatusLine(label: "Session", value: "available", role: StatusRole::Available, dimmed)
                        StatusLine(label: "Directory", value: "warning · demo path", role: StatusRole::Warning, dimmed)
                        StatusLine(label: "Connection", value: "error · simulated", role: StatusRole::Error, dimmed)
                    }
                    View(height: 1) {}
                    View(height: 1, flex_direction: FlexDirection::Row, gap: 3) {
                        ActionButton(label: "Close", width: 10u16, focused: *focus == DetailsFocus::Close, dimmed)
                        ActionButton(label: "Open session", width: 16u16, focused: *focus == DetailsFocus::Open, primary: true, dimmed)
                        ActionButton(label: "Fork session", width: 16u16, focused: *focus == DetailsFocus::Fork, dimmed)
                    }
                }
            }.into_any()
        }
        DemoOverlay::Help => element! {
            View(flex_direction: FlexDirection::Column, padding_left: 2, padding_right: 2) {
                #(Some(shell_text("Picker shell · fake data", dimmed)))
                #(Some(shell_text("Enter details · Ctrl+Enter open", dimmed)))
                #(Some(shell_text("Ctrl+Shift+Enter fork · Ctrl+C quit", dimmed)))
                #(Some(shell_text("F10 menu · Ctrl+N new · / filter", dimmed)))
            }
        }.into_any(),
    };
    element! {
        View(position: Position::Absolute, left: i32::from(rect.left.saturating_sub(1)), top: i32::from(rect.top),
            width: u32::from(rect.width + 1), height: u32::from(rect.height), flex_direction: FlexDirection::Row) {
            View(width: 1, height: u32::from(rect.height), flex_shrink: 0.0_f32, background_color: BackgroundRole::Base.color(true)) {}
            View(width: u32::from(rect.width), height: u32::from(rect.height), flex_shrink: 0.0_f32,
                flex_direction: FlexDirection::Column, background_color: BackgroundRole::Surface.color(dimmed), border_style: BorderStyle::Single,
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
    dropdown: bool,
    dimmed: bool,
}
#[component]
fn DialogFieldRow(props: &DialogFieldRowProps) -> impl Into<AnyElement<'static>> {
    element!(View(height: 1, flex_direction: FlexDirection::Row,
        background_color: props.focused.then_some(BackgroundRole::Focused.color(props.dimmed))) {
        View(width: 12, flex_shrink: 0.0_f32) { #(Some(shell_text(props.label.clone(), props.dimmed))) }
        #(Some(shell_text(if props.dropdown { format!("{} ▾", props.value) } else { props.value.clone() }, props.dimmed)))
    })
}
