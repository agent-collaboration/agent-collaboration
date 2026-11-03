//! Query changes and outside updates have deliberately different highlight rules.
use crate::controls::{
    fields::TextFieldState,
    list::option_list::{DuplicateOptionKey, OptionKey, SelectOption, validate_options},
};
use crate::surfaces::focus_model::InputUse;
use iocraft::prelude::*;
#[derive(Clone, Debug, Eq, PartialEq)]
enum OptionHighlight {
    Key(OptionKey),
    Empty,
    RemovedExternally,
}
impl OptionHighlight {
    fn key(&self) -> Option<&OptionKey> {
        match self {
            Self::Key(key) => Some(key),
            Self::Empty | Self::RemovedExternally => None,
        }
    }
    fn from_key(key: Option<OptionKey>) -> Self {
        key.map_or(Self::Empty, Self::Key)
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComboBoxState {
    options: Vec<SelectOption>,
    value: Option<OptionKey>,
    highlight: OptionHighlight,
    query: TextFieldState,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComboTransition {
    Unused,
    Changed,
    Picked(OptionKey),
}
impl ComboBoxState {
    pub fn new(
        options: Vec<SelectOption>,
        value: Option<OptionKey>,
    ) -> Result<Self, DuplicateOptionKey> {
        validate_options(&options)?;
        Ok(Self {
            options,
            value,
            highlight: OptionHighlight::Empty,
            query: TextFieldState::default(),
        })
    }
    pub fn options(&self) -> &[SelectOption] {
        &self.options
    }
    pub fn value(&self) -> Option<&OptionKey> {
        self.value.as_ref()
    }
    pub fn highlight(&self) -> Option<&OptionKey> {
        self.highlight.key()
    }
    pub fn query(&self) -> &TextFieldState {
        &self.query
    }
    pub fn matching(&self) -> Vec<&SelectOption> {
        self.options
            .iter()
            .filter(|option| option.matches(self.query.text()))
            .collect()
    }
    pub fn begin(&mut self) {
        self.query.clear();
        self.highlight = OptionHighlight::from_key(
            self.value
                .as_ref()
                .filter(|key| self.options.iter().any(|option| option.key == **key))
                .cloned()
                .or_else(|| self.options.first().map(|option| option.key.clone())),
        );
    }
    pub fn replace_options(
        &mut self,
        options: Vec<SelectOption>,
    ) -> Result<(), DuplicateOptionKey> {
        validate_options(&options)?;
        self.options = options;
        if self.highlight.key().is_some()
            && !self.options.iter().any(|option| {
                Some(&option.key) == self.highlight.key() && option.matches(self.query.text())
            })
        {
            self.highlight = OptionHighlight::RemovedExternally;
        }
        Ok(())
    }
    pub fn hover(&mut self, key: &OptionKey) {
        if self.matching().iter().any(|option| option.key == *key) {
            self.highlight = OptionHighlight::Key(key.clone());
        }
    }
    pub fn pick(&mut self) -> Option<OptionKey> {
        let key = self
            .matching()
            .iter()
            .find(|option| {
                Some(&option.key) == self.highlight.key() && option.availability.is_available()
            })
            .map(|option| option.key.clone())?;
        self.value = Some(key.clone());
        Some(key)
    }
    pub fn navigate(&mut self, forward: bool) {
        let matching = self.matching();
        let index = matching
            .iter()
            .position(|option| Some(&option.key) == self.highlight.key());
        let index = match (forward, index) {
            (true, Some(index)) => (index + 1).min(matching.len().saturating_sub(1)),
            (false, Some(index)) => index.saturating_sub(1),
            (true, None) => 0,
            (false, None) => matching.len().saturating_sub(1),
        };
        self.highlight =
            OptionHighlight::from_key(matching.get(index).map(|option| option.key.clone()));
    }
    pub fn handle_event(&mut self, event: &TerminalEvent) -> ComboTransition {
        if let TerminalEvent::Key(key) = event {
            match key.code {
                KeyCode::Down if key.kind != KeyEventKind::Release && key.modifiers.is_empty() => {
                    self.navigate(true);
                    return ComboTransition::Changed;
                }
                KeyCode::Up if key.kind != KeyEventKind::Release && key.modifiers.is_empty() => {
                    self.navigate(false);
                    return ComboTransition::Changed;
                }
                KeyCode::Enter if key.kind == KeyEventKind::Press && key.modifiers.is_empty() => {
                    return self
                        .pick()
                        .map_or(ComboTransition::Changed, ComboTransition::Picked);
                }
                _ => {}
            }
        }
        let before = self.query.text().to_owned();
        if self.query.handle_event(event) == InputUse::Unused {
            return ComboTransition::Unused;
        }
        if self.query.text() != before
            && self.highlight != OptionHighlight::RemovedExternally
            && !self
                .matching()
                .iter()
                .any(|option| Some(&option.key) == self.highlight.key())
        {
            self.highlight =
                OptionHighlight::from_key(self.matching().first().map(|option| option.key.clone()));
        }
        ComboTransition::Changed
    }
}
