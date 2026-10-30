use super::demo_shell_state::{DialogChoice, MachineChoice};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChoiceAvailability {
    Available,
    Unavailable { reason: &'static str },
}
#[derive(Clone, Debug)]
pub struct ChoiceRow {
    pub key: DialogChoice,
    pub availability: ChoiceAvailability,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoiceHighlight {
    Item(DialogChoice),
    AwaitingNavigation,
    NoMatches,
}
#[derive(Clone, Debug)]
pub struct ChoicePickerState {
    pub anchor: DialogChoice,
    pub query: String,
    pub highlight: ChoiceHighlight,
    pub options: Vec<ChoiceRow>,
}
impl ChoicePickerState {
    pub fn new(anchor: DialogChoice) -> Self {
        let options = anchor
            .matching("")
            .into_iter()
            .map(|key| ChoiceRow {
                key,
                availability: if key == DialogChoice::Machine(MachineChoice::BuildBox) {
                    ChoiceAvailability::Unavailable { reason: "offline" }
                } else {
                    ChoiceAvailability::Available
                },
            })
            .collect();
        let mut picker = Self {
            anchor,
            query: String::new(),
            highlight: ChoiceHighlight::Item(anchor),
            options: Vec::new(),
        };
        picker.replace_options(options);
        picker
    }
    pub fn visible_options(&self) -> Vec<&ChoiceRow> {
        self.options
            .iter()
            .filter(|option| {
                option
                    .key
                    .label()
                    .to_lowercase()
                    .contains(&self.query.to_lowercase())
            })
            .collect()
    }
    pub fn highlighted_key(&self) -> Option<DialogChoice> {
        match self.highlight {
            ChoiceHighlight::Item(key) => Some(key),
            ChoiceHighlight::AwaitingNavigation | ChoiceHighlight::NoMatches => None,
        }
    }
    pub fn pickable_key(&self) -> Option<DialogChoice> {
        self.visible_options()
            .into_iter()
            .find(|option| {
                Some(option.key) == self.highlighted_key()
                    && option.availability == ChoiceAvailability::Available
            })
            .map(|option| option.key)
    }
    pub fn replace_options(&mut self, options: Vec<ChoiceRow>) {
        self.options = options;
        if let Some(key) = self.highlighted_key()
            && !self
                .visible_options()
                .iter()
                .any(|option| option.key == key)
        {
            self.highlight = ChoiceHighlight::AwaitingNavigation;
        }
    }
    fn reconcile_own_query(&mut self) {
        if self.highlight == ChoiceHighlight::AwaitingNavigation {
            return;
        }
        let matching = self.visible_options();
        if !matching
            .iter()
            .any(|option| Some(option.key) == self.highlighted_key())
        {
            self.highlight = matching
                .first()
                .map_or(ChoiceHighlight::NoMatches, |option| {
                    ChoiceHighlight::Item(option.key)
                });
        }
    }
    pub fn append_query(&mut self, text: &str) {
        self.query
            .extend(text.chars().filter(|character| !character.is_control()));
        self.reconcile_own_query();
    }
    pub fn backspace_query(&mut self) {
        self.query.pop();
        self.reconcile_own_query();
    }
    pub fn move_highlight(&mut self, down: bool) {
        let options = self.visible_options();
        let current = options
            .iter()
            .position(|option| Some(option.key) == self.highlighted_key());
        let next = match (down, current) {
            (true, Some(index)) => index.saturating_add(1).min(options.len().saturating_sub(1)),
            (false, Some(index)) => index.saturating_sub(1),
            (_, None) => 0,
        };
        self.highlight = options
            .get(next)
            .map_or(ChoiceHighlight::NoMatches, |option| {
                ChoiceHighlight::Item(option.key)
            });
    }
}
