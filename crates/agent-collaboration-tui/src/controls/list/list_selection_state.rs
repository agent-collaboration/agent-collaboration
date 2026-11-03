//! Selection follows an opaque row identity through navigation and reloads.
use crate::actions::{IdentifierError, validate_identifier};
use iocraft::KeyCode;
use std::{
    collections::{BTreeMap, HashSet},
    fmt,
};

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
    BlankGroupHeading {
        group: usize,
    },
    GroupCountMismatch {
        group: usize,
        declared: usize,
        actual: usize,
    },
}
impl fmt::Display for ListSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid list selection: {self:?}")
    }
}
impl std::error::Error for ListSelectionError {}

/// Complete ordered input, replaced atomically when the consumer regroups or reloads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListRows {
    Ungrouped(Vec<ListRowKey>),
    Grouped(Vec<ListGroup>),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListGroupHeading {
    pub name: String,
    pub count: usize,
    pub summary: Option<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListGroup {
    pub heading: ListGroupHeading,
    pub keys: Vec<ListRowKey>,
}
/// The only selectable ordering. Headings annotate boundaries, never add identities.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ListProjection {
    keys: Vec<ListRowKey>,
    headings: BTreeMap<usize, ListGroupHeading>,
    grouped: bool,
}
impl ListProjection {
    fn new(rows: ListRows) -> Result<Self, ListSelectionError> {
        let grouped = matches!(&rows, ListRows::Grouped(_));
        let mut headings = BTreeMap::new();
        let keys = match rows {
            ListRows::Ungrouped(keys) => keys,
            ListRows::Grouped(groups) => {
                let mut keys = Vec::new();
                for (group, entry) in groups.into_iter().enumerate() {
                    if entry.heading.name.trim().is_empty() {
                        return Err(ListSelectionError::BlankGroupHeading { group });
                    }
                    if entry.heading.count != entry.keys.len() {
                        return Err(ListSelectionError::GroupCountMismatch {
                            group,
                            declared: entry.heading.count,
                            actual: entry.keys.len(),
                        });
                    }
                    if !entry.keys.is_empty() {
                        headings.insert(keys.len(), entry.heading);
                        keys.extend(entry.keys);
                    }
                }
                keys
            }
        };
        validate_rows(&keys)?;
        Ok(Self {
            keys,
            headings,
            grouped,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListSelectionState {
    projection: ListProjection,
    selected: Option<ListRowKey>,
}
impl ListSelectionState {
    pub fn new(rows: ListRows, selected: Option<ListRowKey>) -> Result<Self, ListSelectionError> {
        let projection = ListProjection::new(rows)?;
        let rows = &projection.keys;
        if let Some(key) = &selected
            && !rows.contains(key)
        {
            return Err(ListSelectionError::SelectedKeyMissing(key.clone()));
        }
        let selected = selected.or_else(|| rows.first().cloned());
        Ok(Self {
            projection,
            selected,
        })
    }
    pub fn rows(&self) -> &[ListRowKey] {
        &self.projection.keys
    }
    pub(crate) fn is_grouped(&self) -> bool {
        self.projection.grouped
    }
    pub(crate) fn heading_before(&self, row: &ListRowKey) -> Option<&ListGroupHeading> {
        let index = self.projection.keys.iter().position(|key| key == row)?;
        self.projection.headings.get(&index)
    }
    pub fn selected(&self) -> Option<&ListRowKey> {
        self.selected.as_ref()
    }
    pub fn replace_rows(&mut self, rows: ListRows) -> Result<(), ListSelectionError> {
        let projection = ListProjection::new(rows)?;
        let rows = &projection.keys;
        let previous_position = self
            .projection
            .keys
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
        self.projection = projection;
        Ok(())
    }
    pub fn select(&mut self, key: &ListRowKey) -> bool {
        if self.projection.keys.contains(key) && self.selected.as_ref() != Some(key) {
            self.selected = Some(key.clone());
            true
        } else {
            false
        }
    }
    // Before a grouped draw, the callback budget counts new projection slots.
    // The headings have no selectable identity, even when the budget ends on one.
    fn grouped_page(&self, current: usize, budget: usize, forward: bool) -> usize {
        let slots = (0..self.rows().len())
            .map(|index| index + self.projection.headings.range(..=index).count())
            .collect::<Vec<_>>();
        let Some(start) = slots.get(current).copied() else {
            return current;
        };
        let last = slots.len().saturating_sub(1);
        if forward {
            let end = start.saturating_add(budget.max(1));
            slots
                .iter()
                .rposition(|slot| *slot <= end)
                .unwrap_or(current)
                .max(current.saturating_add(1).min(last))
        } else {
            let end = start.saturating_sub(budget.max(1));
            slots
                .iter()
                .position(|slot| *slot >= end)
                .unwrap_or(current)
                .min(current.saturating_sub(1))
        }
    }
    pub(crate) fn navigate(&mut self, code: KeyCode, page_rows: usize) -> bool {
        let current = self
            .projection
            .keys
            .iter()
            .position(|key| Some(key) == self.selected.as_ref())
            .unwrap_or(0);
        let last = self.projection.keys.len().saturating_sub(1);
        let next = match code {
            KeyCode::Up => current.saturating_sub(1),
            KeyCode::Down => current.saturating_add(1).min(last),
            KeyCode::PageUp if self.is_grouped() => self.grouped_page(current, page_rows, false),
            KeyCode::PageUp => current.saturating_sub(page_rows.max(1)),
            KeyCode::PageDown if self.is_grouped() => self.grouped_page(current, page_rows, true),
            KeyCode::PageDown => current.saturating_add(page_rows.max(1)).min(last),
            KeyCode::Home => 0,
            KeyCode::End => last,
            _ => return false,
        };
        let selected = self.projection.keys.get(next).cloned();
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
