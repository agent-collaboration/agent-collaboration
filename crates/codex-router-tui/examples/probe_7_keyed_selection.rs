//! Existing SearchSelect identity behavior under caller-owned list changes.
//! The small query wrapper is an explicit probe policy, not a product change.
use codex_router_tui::{ChoiceOption, ChoiceTransition, SearchSelect, SearchSelectState};
use iocraft::prelude::*;
use std::{fs, io};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChoiceKey {
    One,
    Two,
    Three,
    Four,
    Five,
    Six,
}
fn initial_options() -> Vec<ChoiceOption<ChoiceKey>> {
    [
        (ChoiceKey::One, "Item 1 shared"),
        (ChoiceKey::Two, "Item 2 shared"),
        (ChoiceKey::Three, "Item 3 shared"),
        (ChoiceKey::Four, "Item 4 shared"),
        (ChoiceKey::Five, "Item 5 shared"),
    ]
    .into_iter()
    .map(|(key, label)| ChoiceOption::available(key, label))
    .collect()
}
fn checkpoint(
    label: &str,
    state: &SearchSelectState<ChoiceKey>,
    options: &[ChoiceOption<ChoiceKey>],
) -> io::Result<()> {
    let canvas = element! {
        View(width: 64, height: 12, flex_direction: FlexDirection::Column) {
            Text(content: format!("{label}: effective={:?}", state.available_focused_key(options)))
            SearchSelect<ChoiceKey>(state: state.clone(), options: options.to_vec())
        }
    }
    .render(Some(64));
    let text = format!(
        "CASE {label}\nstate={state:?}\neffective={:?}\norder={:?}\n{canvas}",
        state.available_focused_key(options),
        options.iter().map(|option| option.key).collect::<Vec<_>>()
    );
    println!("{text}");
    fs::write(format!("tmp/probes/7/{label}.txt"), text)?;
    let mut ansi = Vec::new();
    canvas.write_ansi(&mut ansi)?;
    fs::write(format!("tmp/probes/7/{label}.ansi"), ansi)
}
fn append_query_preserving_matching_identity(
    state: &mut SearchSelectState<ChoiceKey>,
    text: &str,
    options: &[ChoiceOption<ChoiceKey>],
) {
    let previous = match state {
        SearchSelectState::Closed => None,
        SearchSelectState::Searching { focused, .. } => *focused,
    };
    state.append_query(text, options);
    if let SearchSelectState::Searching { query, .. } = state {
        let candidate = SearchSelectState::Searching {
            query: query.clone(),
            focused: previous,
        };
        // Reuse the actual library's eligibility rule; no copied filter predicate.
        if candidate.available_focused_key(options).is_some() {
            *state = candidate;
        }
    }
}
fn require_observation(condition: bool, message: &str) -> io::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message))
    }
}
fn main() -> io::Result<()> {
    fs::create_dir_all("tmp/probes/7")?;
    let mut options = initial_options();
    let mut state = SearchSelectState::Closed;
    state.open(ChoiceKey::Three);
    checkpoint("01-item3-selected", &state, &options)?;
    options.reverse();
    checkpoint("02-reordered", &state, &options)?;
    require_observation(
        state.available_focused_key(&options) == Some(ChoiceKey::Three),
        "reorder lost selected identity",
    )?;
    options.retain(|option| option.key != ChoiceKey::Three);
    checkpoint("03-selected-item-removed", &state, &options)?;
    require_observation(
        state.available_focused_key(&options).is_none(),
        "missing identity must not become a different committed choice",
    )?;
    require_observation(
        state.handle_key(KeyCode::Enter, &options) == ChoiceTransition::Handled,
        "missing selected identity must not commit",
    )?;
    state.handle_key(KeyCode::Down, &options);
    checkpoint("04-explicit-navigation-after-removal", &state, &options)?;
    require_observation(
        state.available_focused_key(&options) == Some(ChoiceKey::Five),
        "navigation should select the first current matching row when the old identity disappeared",
    )?;
    options.insert(0, ChoiceOption::available(ChoiceKey::Six, "Item 6 shared"));
    checkpoint("05-external-items-arrive", &state, &options)?;
    require_observation(
        state.available_focused_key(&options) == Some(ChoiceKey::Five),
        "external list update lost an existing identity",
    )?;
    let mut preserved = state.clone();
    state.append_query("Item", &options);
    checkpoint("06-native-filter-matching-key", &state, &options)?;
    require_observation(
        state.available_focused_key(&options) == Some(ChoiceKey::Six),
        "native append_query was expected to choose first match, not preserve Five",
    )?;
    append_query_preserving_matching_identity(&mut preserved, "Item", &options);
    checkpoint("07-preserve-matching-wrapper", &preserved, &options)?;
    require_observation(
        preserved.available_focused_key(&options) == Some(ChoiceKey::Five),
        "probe wrapper lost a still matching identity",
    )?;
    append_query_preserving_matching_identity(&mut preserved, " missing", &options);
    checkpoint("08-no-matches", &preserved, &options)?;
    require_observation(
        preserved.available_focused_key(&options).is_none(),
        "no-match filter retained an eligible selection",
    )?;
    println!(
        "DECISIONS: automatic fallback after removal is not supplied by SearchSelectState; choose no selection, first, or nearest explicitly. Native append_query resets to first match; preserving a still-matching identity requires a caller policy such as the probe wrapper."
    );
    Ok(())
}
