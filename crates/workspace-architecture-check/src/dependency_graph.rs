use std::collections::{BTreeMap, BTreeSet};

use crate::ArchitectureError;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize)]
pub struct ModuleEdge {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ItemEdge {
    pub from: String,
    pub to: String,
}

/// Read the quoted, single-line edge records emitted by cargo-modules 0.28.
/// Other DOT statements are ignored; malformed edge records fail closed.
pub fn parse_use_edges(dot: &str) -> Result<Vec<ItemEdge>, ArchitectureError> {
    if !dot.trim_start().starts_with("digraph {") || !dot.trim_end().ends_with('}') {
        return Err(ArchitectureError::InvalidDot {
            line_number: 1,
            line: "expected a complete digraph".to_owned(),
        });
    }
    let mut edges = BTreeSet::new();
    for (line_index, line) in dot.lines().enumerate() {
        let invalid = || ArchitectureError::InvalidDot {
            line_number: line_index + 1,
            line: line.to_owned(),
        };
        let mut remaining = line.trim();
        if !remaining.starts_with('"') {
            if remaining.contains("label=\"uses\"") {
                return Err(invalid());
            }
            continue;
        }
        let from = take_quoted(&mut remaining).ok_or_else(invalid)?;
        let Some(after_arrow) = remaining.trim_start().strip_prefix("->") else {
            continue;
        };
        remaining = after_arrow.trim_start();
        let to = take_quoted(&mut remaining).ok_or_else(invalid)?;
        let label = remaining.split(',').find_map(|attribute| {
            let (key, value) = attribute.trim().trim_start_matches('[').split_once('=')?;
            if key.trim() != "label" {
                return None;
            }
            take_quoted(&mut value.trim_start())
        });
        match label.as_deref() {
            Some("uses") => {
                edges.insert(ItemEdge { from, to });
            }
            Some(_) => {}
            None => return Err(invalid()),
        }
    }
    Ok(edges.into_iter().collect())
}

fn take_quoted(input: &mut &str) -> Option<String> {
    let mut characters = input.char_indices();
    if characters.next()?.1 != '"' {
        return None;
    }
    let mut escaped = false;
    for (index, character) in characters {
        if character == '"' && !escaped {
            let end = index + 1;
            let value = serde_json::from_str(input.get(..end)?).ok()?;
            *input = input.get(end..)?;
            return Some(value);
        }
        escaped = character == '\\' && !escaped;
    }
    None
}

type ModuleEdges = BTreeMap<String, BTreeSet<String>>;

#[derive(Clone, Debug)]
pub struct ModuleGraph {
    module_edges: ModuleEdges,
    concrete_edges: BTreeMap<(String, String), BTreeSet<ItemEdge>>,
}

impl ModuleGraph {
    pub fn from_dot(dot: &str, crate_name: &str) -> Result<Self, ArchitectureError> {
        Ok(Self::fold_edges(parse_use_edges(dot)?, crate_name))
    }

    pub fn fold_edges(edges: Vec<ItemEdge>, crate_name: &str) -> Self {
        let mut graph = Self {
            module_edges: BTreeMap::new(),
            concrete_edges: BTreeMap::new(),
        };
        for edge in edges {
            let (Some(from_module), Some(to_module)) = (
                top_level_module(&edge.from, crate_name),
                top_level_module(&edge.to, crate_name),
            ) else {
                continue;
            };
            if from_module == to_module {
                continue;
            }
            graph
                .module_edges
                .entry(from_module.clone())
                .or_default()
                .insert(to_module.clone());
            graph
                .concrete_edges
                .entry((from_module, to_module))
                .or_default()
                .insert(edge);
        }
        graph
    }

    /// Kosaraju's two passes, retaining only nontrivial SCCs in stable order.
    pub fn cycle_components(&self) -> Vec<Vec<String>> {
        let mut reverse_edges = ModuleEdges::new();
        for (from_module, targets) in &self.module_edges {
            reverse_edges.entry(from_module.clone()).or_default();
            for target in targets {
                reverse_edges
                    .entry(target.clone())
                    .or_default()
                    .insert(from_module.clone());
            }
        }
        let mut visited = BTreeSet::new();
        let mut order = Vec::new();
        for module in reverse_edges.keys() {
            visit_order(module, &self.module_edges, &mut visited, &mut order);
        }
        visited.clear();
        let mut components = Vec::new();
        for module in order.into_iter().rev() {
            if visited.contains(&module) {
                continue;
            }
            let mut component = Vec::new();
            visit_order(&module, &reverse_edges, &mut visited, &mut component);
            if component.len() > 1 {
                component.sort();
                components.push(component);
            }
        }
        components.sort();
        components
    }

    pub fn cycle_item_edges(&self, component: &[String]) -> Vec<ItemEdge> {
        self.concrete_edges
            .iter()
            .filter(|((from_module, to_module), _)| {
                component.contains(from_module) && component.contains(to_module)
            })
            .flat_map(|(_, edges)| edges.iter().cloned())
            .collect()
    }

    pub fn module_edges_into(&self, component: &[String]) -> Vec<ModuleEdge> {
        self.concrete_edges
            .keys()
            .filter(|(_, to_module)| component.contains(to_module))
            .map(|(from_module, to_module)| ModuleEdge {
                from: from_module.clone(),
                to: to_module.clone(),
            })
            .collect()
    }

    pub fn item_edges_for(&self, edge: &ModuleEdge) -> Vec<ItemEdge> {
        self.concrete_edges
            .get(&(edge.from.clone(), edge.to.clone()))
            .into_iter()
            .flatten()
            .cloned()
            .collect()
    }
}

fn top_level_module(item: &str, crate_name: &str) -> Option<String> {
    let tail = item.strip_prefix(crate_name)?.strip_prefix("::")?;
    let module = tail.split("::").next().filter(|name| !name.is_empty())?;
    Some(format!("{crate_name}::{module}"))
}

fn visit_order(
    module: &str,
    edges: &ModuleEdges,
    visited: &mut BTreeSet<String>,
    order: &mut Vec<String>,
) {
    if !visited.insert(module.to_owned()) {
        return;
    }
    for next in edges.get(module).into_iter().flatten() {
        visit_order(next, edges, visited, order);
    }
    order.push(module.to_owned());
}

#[cfg(test)]
mod tests;
