//! Selection follows an opaque row identity through navigation and reloads.
use crate::actions::{IdentifierError, validate_identifier};
use iocraft::KeyCode;
use std::{collections::HashSet, fmt};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ListRowKey(String);
impl ListRowKey {
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        validate_identifier(&value)?;
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListSelectionError {
    DuplicateKey(ListRowKey),
    SelectedKeyMissing(ListRowKey),
}
impl fmt::Display for ListSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid list selection: {self:?}")
    }
}
impl std::error::Error for ListSelectionError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListSelectionState {
    rows: Vec<ListRowKey>,
    selected: Option<ListRowKey>,
}
impl ListSelectionState {
    pub fn new(
        rows: Vec<ListRowKey>,
        selected: Option<ListRowKey>,
    ) -> Result<Self, ListSelectionError> {
        validate_rows(&rows)?;
        if let Some(key) = &selected
            && !rows.contains(key)
        {
            return Err(ListSelectionError::SelectedKeyMissing(key.clone()));
        }
        let selected = selected.or_else(|| rows.first().cloned());
        Ok(Self { rows, selected })
    }
    pub fn rows(&self) -> &[ListRowKey] {
        &self.rows
    }
    pub fn selected(&self) -> Option<&ListRowKey> {
        self.selected.as_ref()
    }
    pub fn replace_rows(&mut self, rows: Vec<ListRowKey>) -> Result<(), ListSelectionError> {
        validate_rows(&rows)?;
        let previous_position = self
            .rows
            .iter()
            .position(|key| Some(key) == self.selected.as_ref())
            .unwrap_or(0);
        self.selected = self
            .selected
            .as_ref()
            .filter(|key| rows.contains(key))
            .cloned()
            .or_else(|| {
                rows.get(previous_position.min(rows.len().saturating_sub(1)))
                    .cloned()
            });
        self.rows = rows;
        Ok(())
    }
    pub fn select(&mut self, key: &ListRowKey) -> bool {
        if self.rows.contains(key) && self.selected.as_ref() != Some(key) {
            self.selected = Some(key.clone());
            true
        } else {
            false
        }
    }
    pub(crate) fn navigate(&mut self, code: KeyCode, page_rows: usize) -> bool {
        let current = self
            .rows
            .iter()
            .position(|key| Some(key) == self.selected.as_ref())
            .unwrap_or(0);
        let last = self.rows.len().saturating_sub(1);
        let next = match code {
            KeyCode::Up => current.saturating_sub(1),
            KeyCode::Down => current.saturating_add(1).min(last),
            KeyCode::PageUp => current.saturating_sub(page_rows.max(1)),
            KeyCode::PageDown => current.saturating_add(page_rows.max(1)).min(last),
            KeyCode::Home => 0,
            KeyCode::End => last,
            _ => return false,
        };
        let selected = self.rows.get(next).cloned();
        let changed = self.selected != selected;
        self.selected = selected;
        changed
    }
}
fn validate_rows(rows: &[ListRowKey]) -> Result<(), ListSelectionError> {
    let mut seen = HashSet::new();
    for row in rows {
        if !seen.insert(row) {
            return Err(ListSelectionError::DuplicateKey(row.clone()));
        }
    }
    Ok(())
}
