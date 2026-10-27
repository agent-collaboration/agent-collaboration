//! Classify parsed table identities, retaining ambiguous and mixed SQL in the cost.
// Shared by integration binaries with different observation needs.

use std::collections::BTreeSet;
use turso_parser::{ast::Cmd, parser::Parser};

#[derive(Debug, Default, Eq, PartialEq)]
pub struct StatementTables(BTreeSet<String>);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DataStatementCounts {
    pub selects: u64,
    pub inserts: u64,
    pub updates: u64,
    pub deletes: u64,
}
impl DataStatementCounts {
    pub fn total(self) -> u64 {
        self.selects + self.inserts + self.updates + self.deletes
    }
    pub fn count_sql(&mut self, sql: &str) -> Option<u64> {
        use turso_parser::ast::Stmt;
        let mut parser = Parser::new(sql.as_bytes());
        let mut other = 0;
        let mut parsed = false;
        while let Some(command) = parser.next_cmd().ok()? {
            parsed = true;
            let statement = match command {
                Cmd::Stmt(statement)
                | Cmd::Explain(statement)
                | Cmd::ExplainQueryPlan {
                    stmt: statement, ..
                } => statement,
            };
            match statement {
                Stmt::Select(_) => self.selects += 1,
                Stmt::Insert { .. } => self.inserts += 1,
                Stmt::Update(_) => self.updates += 1,
                Stmt::Delete { .. } => self.deletes += 1,
                _ => other += 1,
            }
        }
        parsed.then_some(other)
    }
}

impl StatementTables {
    /// Diagnostics expose only fixed operation and schema labels, never SQL values or names.
    pub fn diagnostic_kind(statement: &str) -> String {
        use turso_parser::ast::Stmt;
        let mut parser = Parser::new(statement.as_bytes());
        let Ok(Some(command)) = parser.next_cmd() else {
            return "unparsed".to_owned();
        };
        let (Cmd::Stmt(statement)
        | Cmd::Explain(statement)
        | Cmd::ExplainQueryPlan {
            stmt: statement, ..
        }) = command;
        let operation = match &statement {
            Stmt::Select(_) => "select",
            Stmt::Insert { .. } => "insert",
            Stmt::Update(_) => "update",
            Stmt::Delete { .. } => "delete",
            Stmt::Begin { .. } => "begin",
            Stmt::Commit { .. } => "commit",
            Stmt::Rollback { .. } => "rollback",
            _ => "other",
        };
        let mut tables = Self::default();
        if let Ok(node) = serde_json::to_value(statement) {
            tables.collect(&node);
        }
        let known = [
            "actors",
            "board_intent_progress",
            "board_intent_scan_cursors",
            "cached_fleet_records",
            "epoch_lines",
            "facts",
            "fleet_cache_checkpoint",
            "fleet_records",
            "handoff_frontier",
            "mail_cancel_expansions",
            "mail_handoffs",
            "notify_intents",
            "posts",
            "project_feed",
            "project_state",
            "request_decisions",
            "router_pushes",
            "sqlite_master",
            "sqlite_schema",
            "subscription_delivery_progress",
            "subscription_memberships",
            "thread_watches",
            "workstreams",
        ];
        let labels: Vec<_> = tables
            .0
            .iter()
            .map(|table| {
                known
                    .iter()
                    .copied()
                    .find(|known| table == known)
                    .unwrap_or("other-table")
            })
            .collect();
        format!("{operation} {labels:?}")
    }

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
