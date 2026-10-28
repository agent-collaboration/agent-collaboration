//! Invocation-local machine browsing and NEW choice; configured routes require qualification.
use super::{PickerSourceContext, SourceInventoryRejection};
use crate::sessions::{RouterRegistryError, RouterRegistryRead};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum PickerMachineSourceMode {
    #[default]
    HostedDefault,
    LocalCodex,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) enum PickerMachineFilter {
    #[default]
    Default,
    Single {
        source: Box<PickerSourceContext>,
    },
    All,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MachineChoicePurpose {
    Browse,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) enum PickerMachineStage {
    #[default]
    Browsing,
    Choosing {
        purpose: MachineChoicePurpose,
        focus: usize,
        notice: Option<MachineChoiceNotice>,
    },
    Loading {
        source: Box<PickerSourceContext>,
        focus: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum MachineChoiceNotice {
    Registry(RouterRegistryError),
    SourceNotQualified(String),
    SourceReadFailed {
        name: String,
        reason: SourceInventoryRejection,
    },
}

impl MachineChoiceNotice {
    pub(super) fn message(&self) -> String {
        match self {
            Self::Registry(error) => error.to_string(),
            Self::SourceNotQualified(name) => {
                format!("{name}: machine connection needs verification; previous view retained")
            }
            Self::SourceReadFailed { name, reason } => {
                format!("{name}: {reason}; previous view retained")
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum MachineChoiceKey {
    Default,
    All,
    Configured(PickerSourceContext),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct PickerMachineControls {
    pub(super) filter: PickerMachineFilter,
    pub(super) stage: PickerMachineStage,
    pub(super) control_focused: bool,
    pub(super) search: codex_router_tui::SearchSelectState<MachineChoiceKey>,
}

impl PickerMachineControls {
    pub(super) fn label(&self, mode: PickerMachineSourceMode) -> &str {
        match (mode, &self.filter) {
            (PickerMachineSourceMode::LocalCodex, _) => "Local Codex",
            (_, PickerMachineFilter::Default) => "This machine",
            (_, PickerMachineFilter::All) => "All machines",
            (_, PickerMachineFilter::Single { source }) => match source.as_ref() {
                PickerSourceContext::ConfiguredHosted(profile) => profile.name.as_str(),
                PickerSourceContext::DefaultHosted => "This machine",
                PickerSourceContext::LocalCodex => "Local Codex",
            },
        }
    }

    pub(super) fn open_browse(&mut self, registry: &RouterRegistryRead) {
        self.search.open(MachineChoiceKey::Default);
        self.stage = PickerMachineStage::Choosing {
            purpose: MachineChoicePurpose::Browse,
            focus: 0,
            notice: registry_notice(registry),
        };
    }

    pub(super) fn choice_options(
        &self,
        registry: &RouterRegistryRead,
        mode: PickerMachineSourceMode,
    ) -> Vec<codex_router_tui::ChoiceOption<MachineChoiceKey>> {
        use codex_router_tui::ChoiceOption;
        let mut choices = vec![ChoiceOption::available(
            MachineChoiceKey::Default,
            match mode {
                PickerMachineSourceMode::HostedDefault => "This machine (current/default)",
                PickerMachineSourceMode::LocalCodex => "Local Codex",
            },
        )];
        if mode == PickerMachineSourceMode::HostedDefault
            && let RouterRegistryRead::Ready(registry) = registry
        {
            if !registry.routers.is_empty() {
                choices.push(ChoiceOption::available(
                    MachineChoiceKey::All,
                    "All machines",
                ));
            }
            choices.extend(registry.routers.iter().map(|profile| {
                ChoiceOption::available(
                    MachineChoiceKey::Configured(PickerSourceContext::ConfiguredHosted(
                        profile.clone(),
                    )),
                    profile.name.as_str(),
                )
            }));
        }
        choices
    }

    pub(super) fn cancel_choice(&mut self) {
        self.search = codex_router_tui::SearchSelectState::Closed;
        self.stage = PickerMachineStage::Browsing;
        self.control_focused = false;
    }

    /// Returns true only for a concrete, established default NEW action.
    pub(super) fn select_choice(
        &mut self,
        registry: &RouterRegistryRead,
        mode: PickerMachineSourceMode,
        can_load_source: bool,
    ) -> bool {
        let PickerMachineStage::Choosing { purpose, focus, .. } = self.stage else {
            return false;
        };
        if focus == 0 {
            self.return_to_default(mode, can_load_source);
            return false;
        }
        let has_all = mode == PickerMachineSourceMode::HostedDefault
            && matches!(purpose, MachineChoicePurpose::Browse)
            && matches!(registry, RouterRegistryRead::Ready(registry) if !registry.routers.is_empty());
        if has_all && focus == 1 {
            self.filter = PickerMachineFilter::All;
            self.cancel_choice();
            return false;
        }
        let profile_index = focus.saturating_sub(if has_all { 2 } else { 1 });
        let RouterRegistryRead::Ready(registry) = registry else {
            return false;
        };
        let Some(profile) = registry.routers.get(profile_index) else {
            return false;
        };
        let name = profile.name.as_str().to_owned();
        if matches!(purpose, MachineChoicePurpose::Browse) && can_load_source {
            self.stage = PickerMachineStage::Loading {
                source: Box::new(PickerSourceContext::ConfiguredHosted(profile.clone())),
                focus,
            };
            return false;
        }
        let notice = MachineChoiceNotice::SourceNotQualified(name);
        self.stage = PickerMachineStage::Choosing {
            purpose,
            focus,
            notice: Some(notice),
        };
        false
    }

    /// A read result never launches; commit only after the worker validates the source tuple.
    pub(super) fn complete_source_switch(&mut self, result: Result<(), SourceInventoryRejection>) {
        let PickerMachineStage::Loading { source, focus } = &self.stage else {
            return;
        };
        match result {
            Ok(()) => {
                self.filter = match source.as_ref() {
                    PickerSourceContext::ConfiguredHosted(_) => PickerMachineFilter::Single {
                        source: source.clone(),
                    },
                    PickerSourceContext::DefaultHosted | PickerSourceContext::LocalCodex => {
                        PickerMachineFilter::Default
                    }
                };
                self.cancel_choice();
            }
            Err(reason) => {
                self.search
                    .open(MachineChoiceKey::Configured(source.as_ref().clone()));
                let name = match source.as_ref() {
                    PickerSourceContext::ConfiguredHosted(profile) => {
                        profile.name.as_str().to_owned()
                    }
                    PickerSourceContext::DefaultHosted => "This machine".to_owned(),
                    PickerSourceContext::LocalCodex => "Local Codex".to_owned(),
                };
                self.stage = PickerMachineStage::Choosing {
                    purpose: MachineChoicePurpose::Browse,
                    focus: *focus,
                    notice: Some(MachineChoiceNotice::SourceReadFailed { name, reason }),
                };
            }
        }
    }

    pub(super) fn return_to_default(
        &mut self,
        mode: PickerMachineSourceMode,
        can_load_source: bool,
    ) {
        if can_load_source && self.filter != PickerMachineFilter::Default {
            self.stage = PickerMachineStage::Loading {
                source: Box::new(match mode {
                    PickerMachineSourceMode::HostedDefault => PickerSourceContext::DefaultHosted,
                    PickerMachineSourceMode::LocalCodex => PickerSourceContext::LocalCodex,
                }),
                focus: 0,
            };
        } else {
            self.filter = PickerMachineFilter::Default;
            self.cancel_choice();
        }
    }

    pub(super) fn all_notice(
        &self,
        registry: &RouterRegistryRead,
        progress: &[super::source_reload_progress::SourceReadProgress],
    ) -> Option<String> {
        if self.filter != PickerMachineFilter::All {
            return None;
        }
        if !progress.is_empty() {
            return super::source_reload_progress::all_read_notice(progress);
        }
        let RouterRegistryRead::Ready(registry) = registry else {
            return None;
        };
        let names = registry
            .routers
            .iter()
            .map(|profile| profile.name.as_str())
            .collect::<Vec<_>>();
        (!names.is_empty()).then(|| {
            format!(
                "Partial view — unqualified: {} (connections need verification)",
                names.join(", ")
            )
        })
    }
}

fn registry_notice(registry: &RouterRegistryRead) -> Option<MachineChoiceNotice> {
    match registry {
        RouterRegistryRead::Rejected(error) => Some(MachineChoiceNotice::Registry(*error)),
        _ => None,
    }
}
