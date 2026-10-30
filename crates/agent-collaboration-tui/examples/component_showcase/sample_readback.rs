//! Wire projections of public observations, separate from sample domain state.
use agent_collaboration_tui::{
    actions::{ActionRegistry, Availability},
    hit_regions::{HitMapSnapshot, HitRegionKind},
    surface_stack::{OverlayKind, SurfaceId, SurfaceModel},
};
use serde::Serialize;
#[derive(Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ObservationCause {
    Input,
    Fixture,
    Frame,
}
#[derive(Serialize)]
#[serde(transparent)]
pub struct SurfaceReadback(String);
impl From<SurfaceId> for SurfaceReadback {
    fn from(surface: SurfaceId) -> Self {
        Self(match surface {
            SurfaceId::Screen => "screen".into(),
            SurfaceId::Overlay(id) => format!("overlay:{}", id.get()),
        })
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SettingReadback {
    All,
    Local,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OverlayKindReadback {
    Menu,
    Dialog,
    Dropdown,
}
impl From<&OverlayKind> for OverlayKindReadback {
    fn from(kind: &OverlayKind) -> Self {
        match kind {
            OverlayKind::Menu => Self::Menu,
            OverlayKind::Dialog => Self::Dialog,
            OverlayKind::Dropdown { .. } => Self::Dropdown,
        }
    }
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AvailabilityReadback {
    Available,
    Unavailable { reason: String },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionReadback {
    pub action_id: String,
    pub title: String,
    pub toolbar_label: Option<String>,
    pub keys: Vec<String>,
    pub availability: AvailabilityReadback,
    pub checked: Option<bool>,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PrimaryReadback {
    Observed { action: Option<String> },
    NotExposed,
}
#[derive(Serialize)]
pub struct RegistryReadback {
    pub scope: String,
    pub primary: PrimaryReadback,
    pub actions: Vec<ActionReadback>,
}
impl RegistryReadback {
    pub fn from_registry(registry: &ActionRegistry) -> Self {
        Self {
            scope: registry.scope().as_str().to_owned(),
            primary: PrimaryReadback::NotExposed,
            actions: registry
                .presentations()
                .into_iter()
                .map(|action| ActionReadback {
                    action_id: action.id.as_str().to_owned(),
                    title: action.title,
                    toolbar_label: action.toolbar_label,
                    keys: action.keys.into_iter().map(|key| key.to_string()).collect(),
                    availability: match action.availability {
                        Availability::Available => AvailabilityReadback::Available,
                        Availability::Unavailable { reason } => {
                            AvailabilityReadback::Unavailable { reason }
                        }
                    },
                    checked: action.checked,
                })
                .collect(),
        }
    }
    pub fn from_model(model: &SurfaceModel) -> Self {
        let mut registry = Self::from_registry(&model.actions);
        registry.primary = PrimaryReadback::Observed {
            action: model
                .primary_action
                .as_ref()
                .map(|action| action.as_str().to_owned()),
        };
        registry
    }
}
#[derive(Serialize)]
pub struct OverlayRegistryReadback {
    pub surface: SurfaceReadback,
    pub current: RegistryReadback,
    pub base: Option<RegistryReadback>,
}
#[derive(Serialize)]
pub struct RegistriesReadback {
    pub screen: Option<RegistryReadback>,
    pub overlays: Vec<OverlayRegistryReadback>,
}
#[derive(Serialize)]
pub struct EffectReadback {
    pub action: &'static str,
    pub target: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayReadback {
    pub overlay_id: String,
    pub kind: OverlayKindReadback,
    pub primary: Option<String>,
}
#[derive(Serialize)]
pub struct ScrollReadback {
    pub main: Option<u16>,
    pub dialog: Option<u16>,
    pub detail: Option<u16>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HitKindReadback {
    Target,
    ScrollArea,
}
#[derive(Serialize)]
pub struct TargetReadback {
    pub surface: SurfaceReadback,
    pub target: String,
    pub kind: HitKindReadback,
    pub cells: Vec<(u16, u16)>,
}
#[derive(Serialize)]
pub struct SurfaceRegionReadback {
    pub surface: SurfaceReadback,
    pub cells: Vec<(u16, u16)>,
}
#[derive(Serialize)]
pub struct HitMapReadback {
    pub frame: u64,
    pub targets: Vec<TargetReadback>,
    pub surfaces: Vec<SurfaceRegionReadback>,
}
impl From<HitMapSnapshot> for HitMapReadback {
    fn from(map: HitMapSnapshot) -> Self {
        Self {
            frame: map.frame,
            targets: map
                .targets
                .into_iter()
                .map(|region| TargetReadback {
                    surface: region.surface.into(),
                    target: region.target.as_str().to_owned(),
                    kind: match region.kind {
                        HitRegionKind::Target => HitKindReadback::Target,
                        HitRegionKind::ScrollArea => HitKindReadback::ScrollArea,
                    },
                    cells: region.cells,
                })
                .collect(),
            surfaces: map
                .surfaces
                .into_iter()
                .map(|region| SurfaceRegionReadback {
                    surface: region.surface.into(),
                    cells: region.cells,
                })
                .collect(),
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateReadback {
    pub observation: u64,
    pub input_ordinal: u64,
    pub cause: ObservationCause,
    pub completion_ready: bool,
    pub selection: Option<String>,
    pub detail_selection: Option<String>,
    pub filter: String,
    pub filter_cursor: usize,
    pub draft: String,
    pub cursor: usize,
    pub committed: String,
    pub choice: Option<String>,
    pub query: String,
    pub highlight: Option<String>,
    pub effects: Vec<EffectReadback>,
    pub setting: SettingReadback,
    pub title: String,
    pub primary_title: String,
    pub start_enabled: bool,
    pub quit_enabled: bool,
    pub alternate_primary: bool,
    pub completion_count: usize,
    pub pending_completion: bool,
    pub surface: SurfaceReadback,
    pub focus: Option<String>,
    pub focus_targets: Vec<FocusTargetReadback>,
    pub overlays: Vec<OverlayReadback>,
    pub scroll: ScrollReadback,
    pub viewport: (u16, u16),
    pub hit_map: Option<HitMapReadback>,
    pub registries: RegistriesReadback,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TargetRoleReadback {
    Focusable,
    PointerOnly,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusTargetReadback {
    pub target_id: String,
    pub role: TargetRoleReadback,
    pub eligible: bool,
}

#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ShowcaseRecord<'a> {
    State {
        snapshot: &'a StateReadback,
    },
    SerializedFrame {
        sequence: u64,
        draw_observation: u64,
        input_ordinal: u64,
    },
}
