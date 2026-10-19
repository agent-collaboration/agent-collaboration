//! Inline typed choice interaction. Callers route it before enclosing actions.
use crate::{SelectableRow, UseDialogInteraction};
use iocraft::prelude::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChoiceAvailability {
    Available,
    Unavailable { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChoiceOption<TKey> {
    pub key: TKey,
    pub label: String,
    pub secondary: Option<String>,
    pub availability: ChoiceAvailability,
}

impl<TKey> ChoiceOption<TKey> {
    pub fn available(key: TKey, label: impl Into<String>) -> Self {
        Self {
            key,
            label: label.into(),
            secondary: None,
            availability: ChoiceAvailability::Available,
        }
    }
    fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.label.to_lowercase().contains(&query)
            || self
                .secondary
                .as_ref()
                .is_some_and(|label| label.to_lowercase().contains(&query))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum SearchSelectState<TKey> {
    #[default]
    Closed,
    Searching {
        query: String,
        focused: Option<TKey>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChoiceTransition<TKey> {
    Unhandled,
    Handled,
    Selected(TKey),
    Cancelled,
}

impl<TKey: Clone + Eq> SearchSelectState<TKey> {
    pub fn open(&mut self, selected: TKey) {
        *self = Self::Searching {
            query: String::new(),
            focused: Some(selected),
        };
    }
    pub fn is_open(&self) -> bool {
        matches!(self, Self::Searching { .. })
    }
    pub fn append_query(&mut self, text: &str, options: &[ChoiceOption<TKey>]) {
        if let Self::Searching { query, focused } = self {
            query.extend(text.chars().filter(|character| !character.is_control()));
            *focused = options
                .iter()
                .find(|option| option.matches(query))
                .map(|option| option.key.clone());
        }
    }
    pub fn handle_key(
        &mut self,
        code: KeyCode,
        options: &[ChoiceOption<TKey>],
    ) -> ChoiceTransition<TKey> {
        let Self::Searching { query, focused } = self else {
            return ChoiceTransition::Unhandled;
        };
        match code {
            KeyCode::Esc => {
                *self = Self::Closed;
                ChoiceTransition::Cancelled
            }
            KeyCode::Enter => {
                let selected = options
                    .iter()
                    .find(|option| {
                        Some(&option.key) == focused.as_ref()
                            && option.matches(query)
                            && option.availability == ChoiceAvailability::Available
                    })
                    .map(|option| option.key.clone());
                if let Some(key) = selected {
                    *self = Self::Closed;
                    ChoiceTransition::Selected(key)
                } else {
                    ChoiceTransition::Handled
                }
            }
            KeyCode::Up | KeyCode::Down => {
                let matching = options
                    .iter()
                    .filter(|option| option.matches(query))
                    .collect::<Vec<_>>();
                let current = matching
                    .iter()
                    .position(|option| Some(&option.key) == focused.as_ref());
                let next = match (code, current) {
                    (KeyCode::Up, Some(index)) => index.saturating_sub(1),
                    (KeyCode::Down, Some(index)) => index
                        .saturating_add(1)
                        .min(matching.len().saturating_sub(1)),
                    _ => 0,
                };
                *focused = matching.get(next).map(|option| option.key.clone());
                ChoiceTransition::Handled
            }
            KeyCode::Char(character) if !character.is_control() => {
                self.append_query(&character.to_string(), options);
                ChoiceTransition::Handled
            }
            KeyCode::Backspace => {
                query.pop();
                if !options
                    .iter()
                    .any(|option| Some(&option.key) == focused.as_ref() && option.matches(query))
                {
                    *focused = options
                        .iter()
                        .find(|option| option.matches(query))
                        .map(|option| option.key.clone());
                }
                ChoiceTransition::Handled
            }
            _ => ChoiceTransition::Handled,
        }
    }
}

#[derive(Props)]
pub struct SearchSelectProps<TKey: Clone + Eq + Send + Sync + 'static> {
    pub state: SearchSelectState<TKey>,
    pub options: Vec<ChoiceOption<TKey>>,
    pub on_select: Handler<TKey>,
}

#[component]
pub fn SearchSelect<TKey: Clone + Eq + Send + Sync + 'static>(
    props: &mut SearchSelectProps<TKey>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let reveal_empty = matches!(
        &props.state,
        SearchSelectState::Searching { focused: None, .. }
    );
    crate::dialog_focus_reveal::use_focus_reveal(&mut hooks, reveal_empty);
    let SearchSelectState::Searching { query, focused } = &props.state else {
        return element!(View).into_any();
    };
    let query = query.clone();
    let enabled = hooks.dialog_input_enabled();
    let handler = props.on_select.clone();
    let options = std::mem::take(&mut props.options)
        .into_iter()
        .filter(|option| option.matches(&query))
        .collect::<Vec<_>>();
    let rows=options.into_iter().map(|option| {
        let selected=Some(&option.key)==focused.as_ref();
        let label=option.label;
        let secondary=option.secondary;
        let (available,reason)=match option.availability { ChoiceAvailability::Available=>(true,None),ChoiceAvailability::Unavailable { reason }=>(false,Some(reason)) };
        let key=option.key;
        let handler=handler.clone();
        element! {
            Button(handler:move |_|if available && enabled { handler(key.clone()); }) {
                View(width:100pct,flex_shrink:0.0_f32) {
                    SelectableRow(selected) {
                        Text(content:label,color:if !available { Color::DarkGrey } else if selected { Color::Yellow } else { Color::White })
                        #(secondary.map(|content|element!(Text(content,color:Color::Grey))))
                        #(reason.map(|content|element!(Text(content,color:Color::DarkGrey))))
                    }
                }
            }
        }.into_any()
    }).collect::<Vec<_>>();
    element! {
        View(flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
            Text(content:format!("Search: {query}"),color:Color::Grey)
            #(rows.is_empty().then(||element!(Text(content:"No matching choices",color:Color::Grey))))
            #(rows)
        }
    }.into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn query_reorder_missing_and_unavailable_do_not_change_committed_identity() {
        let mut choices = vec![
            ChoiceOption::available(10, "東京 local"),
            ChoiceOption::available(20, "Remote"),
        ];
        choices[1].secondary = Some("Sunclaw".to_owned());
        let mut state = SearchSelectState::Closed;
        state.open(20);
        choices.reverse();
        assert_eq!(
            state.handle_key(KeyCode::Enter, &choices),
            ChoiceTransition::Selected(20)
        );
        state.open(99);
        assert_eq!(
            state.handle_key(KeyCode::Enter, &choices),
            ChoiceTransition::Handled
        );
        state.append_query("SUNCLAW", &choices);
        assert_eq!(
            state.handle_key(KeyCode::Enter, &choices),
            ChoiceTransition::Selected(20)
        );
        choices[0].availability = ChoiceAvailability::Unavailable {
            reason: "offline".to_owned(),
        };
        state.open(20);
        assert_eq!(
            state.handle_key(KeyCode::Enter, &choices),
            ChoiceTransition::Handled
        );
        state.append_query("no match", &choices);
        assert_eq!(
            state.handle_key(KeyCode::Enter, &choices),
            ChoiceTransition::Handled
        );
        assert_eq!(
            state.handle_key(KeyCode::Esc, &choices),
            ChoiceTransition::Cancelled
        );
        assert_eq!(
            state.handle_key(KeyCode::Esc, &choices),
            ChoiceTransition::Unhandled
        );
    }
}
