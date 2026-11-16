import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/workspace-rules",
  files: [
    {
      file: "crates/workspace-rules/src/backlog_rules.rs",
      disposition: {
        kind: "none",
      },
      note: "central workspace rule engine (commands and declared rows in, central-storage statement plans out; lib.rs:1 'No IO'); no serde shapes of the four protocols: backlog rules",
    },
    {
      file: "crates/workspace-rules/src/dependency_rules.rs",
      disposition: {
        kind: "none",
      },
      note: "central workspace rule engine (commands and declared rows in, central-storage statement plans out; lib.rs:1 'No IO'); no serde shapes of the four protocols: dependency rules",
    },
    {
      file: "crates/workspace-rules/src/lib.rs",
      disposition: {
        kind: "none",
      },
      note: "central workspace rule engine (commands and declared rows in, central-storage statement plans out; lib.rs:1 'No IO'); no serde shapes of the four protocols: module root; rule_fixtures.rs is cfg(test) at l24-26",
    },
    {
      file: "crates/workspace-rules/src/organization_rules.rs",
      disposition: {
        kind: "none",
      },
      note: "central workspace rule engine (commands and declared rows in, central-storage statement plans out; lib.rs:1 'No IO'); no serde shapes of the four protocols: organization rules",
    },
    {
      file: "crates/workspace-rules/src/read_rules.rs",
      disposition: {
        kind: "none",
      },
      note: "central workspace rule engine (commands and declared rows in, central-storage statement plans out; lib.rs:1 'No IO'); no shapes of the four protocols: read rules; ReadScope encoded as JSON text for a central-storage column via workspace_model::encode_json (l20-40), central-internal",
    },
    {
      file: "crates/workspace-rules/src/reference_rules.rs",
      disposition: {
        kind: "none",
      },
      note: "central workspace rule engine (commands and declared rows in, central-storage statement plans out; lib.rs:1 'No IO'); no shapes of the four protocols: reference rules; reference/link details encoded as JSON text for central-storage columns (encode_json l139, l177), central-internal",
    },
    {
      file: "crates/workspace-rules/src/role_rules.rs",
      disposition: {
        kind: "none",
      },
      note: "central workspace rule engine (commands and declared rows in, central-storage statement plans out; lib.rs:1 'No IO'); no serde shapes of the four protocols: role rules",
    },
    {
      file: "crates/workspace-rules/src/rule_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          'cfg(test) module declared at lib.rs:24-26 (#[cfg(test)] #[path = "rule_fixtures.rs"] mod fixtures)',
      },
      note: "WriteContext test fixtures",
    },
    {
      file: "crates/workspace-rules/src/statement_plans.rs",
      disposition: {
        kind: "none",
      },
      note: "central workspace rule engine (commands and declared rows in, central-storage statement plans out; lib.rs:1 'No IO'); no serde shapes of the four protocols: RulePlan/WriteContext over central_storage statements",
    },
    {
      file: "crates/workspace-rules/src/status_rules.rs",
      disposition: {
        kind: "none",
      },
      note: "central workspace rule engine (commands and declared rows in, central-storage statement plans out; lib.rs:1 'No IO'); no serde shapes of the four protocols: status rules",
    },
    {
      file: "crates/workspace-rules/src/thread_rules.rs",
      disposition: {
        kind: "none",
      },
      note: "central workspace rule engine (commands and declared rows in, central-storage statement plans out; lib.rs:1 'No IO'); no shapes of the four protocols: thread rules; post addressees encoded as JSON text for a central-storage column (encode_json l276), central-internal",
    },
    {
      file: "crates/workspace-rules/src/work_log_rules.rs",
      disposition: {
        kind: "none",
      },
      note: "central workspace rule engine (commands and declared rows in, central-storage statement plans out; lib.rs:1 'No IO'); no serde shapes of the four protocols: work-log rules",
    },
  ],
  sendSites: [],
};
