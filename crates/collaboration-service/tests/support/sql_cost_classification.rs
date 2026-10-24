//! Classify parsed table identities, retaining ambiguous and mixed SQL in the cost.
#![allow(dead_code)] // Shared by integration binaries with different observation needs.

use std::collections::BTreeSet;
use turso_parser::{ast::Cmd, parser::Parser};

#[derive(Debug, Default, Eq, PartialEq)]
pub struct StatementTables(BTreeSet<String>);

impl StatementTables {
    pub fn parse(statement: &str) -> Option<Self> {
        let mut parser = Parser::new(statement.as_bytes());
        let mut tables = Self::default();
        while let Some(command) = parser.next_cmd().ok()? {
            let statement = match command {
                Cmd::Stmt(statement) | Cmd::Explain(statement) => statement,
                Cmd::ExplainQueryPlan { stmt, .. } => stmt,
            };
            // Only data statements can be library bookkeeping. Schema changes,
            // transaction commands and parser failures always remain counted.
            if !matches!(
                statement,
                turso_parser::ast::Stmt::Select(_)
                    | turso_parser::ast::Stmt::Insert { .. }
                    | turso_parser::ast::Stmt::Update(_)
                    | turso_parser::ast::Stmt::Delete { .. }
            ) {
                return None;
            }
            tables.collect(&serde_json::to_value(statement).ok()?);
        }
        Some(tables)
    }

    fn collect(&mut self, node: &serde_json::Value) {
        match node {
            serde_json::Value::Object(fields) => {
                // QualifiedName is the parser's common representation for DML
                // targets, FROM/JOIN sources and IN-table/subquery references.
                // Traverse the whole AST, including nested and compound reads.
                if fields.len() == 3
                    && fields.contains_key("db_name")
                    && fields.contains_key("alias")
                    && let Ok(name) =
                        serde_json::from_value::<turso_parser::ast::QualifiedName>(node.clone())
                {
                    self.0.insert(name.name.as_str().to_ascii_lowercase());
                }
                for value in fields.values() {
                    self.collect(value);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    self.collect(value);
                }
            }
            _ => {}
        }
    }

    pub fn only_library_bookkeeping(&self) -> bool {
        !self.0.is_empty() && self.0.iter().all(|table| table.starts_with("_fleet_sync_"))
    }
}
