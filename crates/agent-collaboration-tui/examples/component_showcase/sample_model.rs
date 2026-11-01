//! Sample-owned values and recording effects; controls own editing and navigation.
use agent_collaboration_tui::controls::{
    HelpContent, HelpContextLine, HelpControl, HelpInputConvention, ListSelectionControl,
};
use agent_collaboration_tui::{
    actions::{ActionDef, ActionId, AvailabilityFn, Placement},
    controls::{
        ComboBoxState, ListRowKey, ListSelectionState, OptionKey, SelectOption, TextFieldState,
    },
    focus_model::TargetId,
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};

pub const FILTER_OUTER_MINIMUM_WIDTH: u16 = 14;
pub const FILTER_SETTINGS_GAP: u16 = 2;

pub fn settings_switch_width(row_width: u16) -> u16 {
    row_width
        .saturating_sub(FILTER_OUTER_MINIMUM_WIDTH)
        .saturating_sub(FILTER_SETTINGS_GAP)
}

#[expect(
    clippy::expect_used,
    reason = "The sample contains fixed validated fixture identities; refusal of a declared control operation must fail the apparatus rather than fabricate fallback state."
)]
pub fn action_id(name: &str) -> ActionId {
    ActionId::new(name).expect("constant action identity")
}
#[expect(
    clippy::expect_used,
    reason = "The sample contains fixed validated fixture identities; refusal of a declared control operation must fail the apparatus rather than fabricate fallback state."
)]
pub fn target_id(name: &str) -> TargetId {
    TargetId::new(name).expect("constant target identity")
}
#[expect(
    clippy::expect_used,
    reason = "The sample contains fixed validated fixture identities; refusal of a declared control operation must fail the apparatus rather than fabricate fallback state."
)]
pub fn option_key(name: &str) -> OptionKey {
    OptionKey::new(name).expect("constant option identity")
}
#[expect(
    clippy::expect_used,
    reason = "The sample contains fixed validated fixture identities; refusal of a declared control operation must fail the apparatus rather than fabricate fallback state."
)]
pub fn row_key(name: &str) -> ListRowKey {
    ListRowKey::new(name).expect("constant row identity")
}
#[expect(
    clippy::expect_used,
    reason = "The sample contains fixed validated fixture identities; refusal of a declared control operation must fail the apparatus rather than fabricate fallback state."
)]
pub fn action(
    name: &'static str,
    title: &'static str,
    keys: &[&str],
    placement: Placement,
    run: impl Fn(()) + Send + Sync + 'static,
) -> ActionDef {
    ActionDef {
        id: action_id(name),
        title: title.into(),
        toolbar_label: None,
        keys: keys
            .iter()
            .map(|key| key.parse().expect("constant key chord"))
            .collect(),
        availability: AvailabilityFn::default(),
        choice: None,
        placement,
        run: Handler::from(run),
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SampleSetting {
    All,
    Local,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClippingCase {
    PlainHorizontal,
    PlainVertical,
    BorderedHorizontal,
    BorderedVertical,
    NestedHorizontal,
    NestedVertical,
}
impl ClippingCase {
    pub fn is_vertical(self) -> bool {
        matches!(
            self,
            Self::PlainVertical | Self::BorderedVertical | Self::NestedVertical
        )
    }
    pub fn uses_multiline_label(self) -> bool {
        self.is_vertical() || self == Self::NestedHorizontal
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoverageGlyph {
    Folder,
    Mail,
    Cjk,
}
impl CoverageGlyph {
    pub fn text(self) -> &'static str {
        match self {
            Self::Folder => "📂",
            Self::Mail => "✉️",
            Self::Cjk => "界",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoverageScene {
    Menu,
    Dialog,
    Dropdown,
    LeftEdge,
}
#[derive(Clone, Copy, Debug)]
pub struct CoverageSeed {
    pub column: u16,
    pub viewport: (u16, u16),
}
#[derive(Clone, Copy, Debug)]
pub struct CoverageFixture {
    pub glyph: CoverageGlyph,
    pub scene: CoverageScene,
    pub seed: Option<CoverageSeed>,
}
impl CoverageFixture {
    pub fn column(self, viewport: (u16, u16)) -> Option<u16> {
        self.seed
            .filter(|seed| seed.viewport == viewport)
            .map(|seed| seed.column)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionKind {
    None,
    UpdateSuspended,
    RemoveHighlight,
    Reorder,
    AddOption,
    EmptyOptions,
    RestoreOptions,
    ReplaceFocusedTarget,
}
#[derive(Clone, Debug)]
pub struct EffectRecord {
    pub action: &'static str,
    pub target: Option<String>,
}
#[derive(Clone, Debug)]
pub struct SampleModel {
    pub coverage: Option<CoverageFixture>,
    pub setting: SampleSetting,
    pub effects: Vec<EffectRecord>,
    pub live_title: String,
    pub primary_title: String,
    pub start_enabled: bool,
    pub quit_enabled: bool,
    pub local_enabled: bool,
    pub filter_enabled: bool,
    pub draft_visible: bool,
    pub clipping_case: ClippingCase,
    pub alternate_primary: bool,
    pub queued_completion: CompletionKind,
    pub completion_count: usize,
}
impl Default for SampleModel {
    fn default() -> Self {
        Self {
            coverage: None,
            setting: SampleSetting::All,
            effects: Vec::new(),
            live_title: "Record exact selection".into(),
            primary_title: "Start".into(),
            start_enabled: true,
            quit_enabled: true,
            local_enabled: true,
            filter_enabled: true,
            draft_visible: true,
            clipping_case: ClippingCase::BorderedHorizontal,
            alternate_primary: false,
            queued_completion: CompletionKind::None,
            completion_count: 0,
        }
    }
}
#[derive(Clone, Debug)]
pub struct SampleRow {
    pub key: ListRowKey,
    pub title: String,
    pub glyph: &'static str,
}
pub fn fixture_rows() -> Vec<SampleRow> {
    (0..36)
        .zip(["📂", "✉️", "界"].into_iter().cycle())
        .map(|(index, glyph)| SampleRow {
            key: row_key(&format!("sample-{index:02}")),
            title: match index {
                0 | 1 => "Same label".into(),
                2 => "界 · 長い名前".into(),
                _ => format!("Sample row {index:02}"),
            },
            glyph,
        })
        .collect()
}
pub fn fixture_options() -> Vec<SelectOption> {
    let mut choices = vec![
        SelectOption::available(option_key("alpha"), "Shared label"),
        SelectOption::available(option_key("beta"), "Shared label"),
        SelectOption::available(option_key("cjk"), "界 · Local choice"),
    ];
    choices.push(SelectOption {
        key: option_key("unavailable"),
        label: "Unavailable choice".into(),
        secondary: Some("Requires sample permission".into()),
        availability: agent_collaboration_tui::actions::Availability::Unavailable {
            reason: "Requires sample permission".into(),
        },
    });
    choices.extend((0..18).map(|index| {
        SelectOption::available(
            option_key(&format!("option-{index:02}")),
            format!("Long choice {index:02}"),
        )
    }));
    choices
}
#[derive(Clone, Copy)]
pub struct SampleData {
    pub model: State<SampleModel>,
    pub filter: State<TextFieldState>,
    pub draft: State<TextFieldState>,
    pub choices: State<ComboBoxState>,
    pub rows: State<Vec<SampleRow>>,
    pub selected: State<ListSelectionState>,
    pub detail_selected: State<ListSelectionState>,
    pub committed: State<String>,
    pub main_scroll: State<Option<agent_collaboration_tui::controls::ScrollHandle>>,
    pub dialog_scroll: State<Option<agent_collaboration_tui::controls::ScrollHandle>>,
    pub detail_control: State<Option<ListSelectionControl>>,
    pub last_overlay: State<Option<agent_collaboration_tui::surface_stack::OverlayId>>,
}
impl SampleData {
    pub fn update(self, update: impl FnOnce(&mut SampleModel)) {
        let mut next = self.model.read().clone();
        update(&mut next);
        let mut model = self.model;
        model.set(next);
    }
    pub fn record(self, name: &'static str) {
        let target = self
            .selected
            .read()
            .selected()
            .map(|key| key.as_str().to_owned());
        self.update(|model| {
            model.effects.push(EffectRecord {
                action: name,
                target,
            })
        });
    }
    pub fn visible_keys(self) -> Vec<ListRowKey> {
        let query = self.filter.read().text().to_lowercase();
        self.rows
            .read()
            .iter()
            .filter(|row| {
                row.title.to_lowercase().contains(&query) || row.key.as_str().contains(&query)
            })
            .map(|row| row.key.clone())
            .collect()
    }
}

#[derive(Clone)]
pub struct ScreenBindings {
    pub list: ListSelectionControl,
    pub help: HelpControl,
}
pub fn help_content(surface: &str) -> HelpContent {
    HelpContent {
        context: vec![HelpContextLine {
            label: "Surface".into(),
            value: surface.into(),
        }],
        conventions: vec![
            HelpInputConvention {
                key: "Tab / Shift+Tab".into(),
                description: "Move between enabled controls".into(),
            },
            HelpInputConvention {
                key: "Enter".into(),
                description: "Inspect; modified Enter records exact selected key".into(),
            },
            HelpInputConvention {
                key: "Esc".into(),
                description: "Close dropdown/dialog; clear filter, reset setting, never quit"
                    .into(),
            },
            HelpInputConvention {
                key: "Ctrl+C".into(),
                description: "Registered Quit owner, subject to committing guard".into(),
            },
            HelpInputConvention {
                key: "Typing / paste".into(),
                description: "Edit the focused field at its Unicode cursor".into(),
            },
        ],
    }
}

#[expect(
    clippy::expect_used,
    reason = "These fixed keyed local fixtures are validated at sample initialization; invalid definitions must fail the apparatus."
)]
pub fn initial_choices() -> ComboBoxState {
    ComboBoxState::new(fixture_options(), Some(option_key("beta"))).expect("unique fixed choices")
}
#[expect(
    clippy::expect_used,
    reason = "These fixed keyed local fixtures are validated at sample initialization; invalid definitions must fail the apparatus."
)]
pub fn initial_list() -> ListSelectionState {
    ListSelectionState::new(
        fixture_rows().into_iter().map(|row| row.key).collect(),
        None,
    )
    .expect("unique fixed rows")
}
#[expect(
    clippy::expect_used,
    reason = "These fixed keyed local fixtures are validated at sample initialization; invalid definitions must fail the apparatus."
)]
pub fn initialized_bindings(handle: &Arc<Mutex<Option<ScreenBindings>>>) -> ScreenBindings {
    handle
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .expect("initialized sample bindings")
}
#[expect(
    clippy::expect_used,
    reason = "These fixed keyed local fixtures are validated at sample initialization; invalid definitions must fail the apparatus."
)]
pub fn visible_row(data: &SampleData, key: &ListRowKey) -> SampleRow {
    let data = *data;
    data.rows
        .read()
        .iter()
        .find(|row| &row.key == key)
        .cloned()
        .expect("row target belongs to fixed fixture")
}
