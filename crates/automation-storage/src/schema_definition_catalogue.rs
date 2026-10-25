//! Exact retained automation tables and indexes after the message-owner cutover.
pub(super) const CURRENT_TARGET_DEFINITION_SOURCE: &str = concat!(
    include_str!("../migrations/20260910000000_automation_v1.sql"),
    "\n",
    include_str!("../migrations/20260928000000_latest_agent_sender.sql"),
    "\n",
    include_str!("../migrations/20261016000100_wake_target_slot.sql"),
    "\n",
    include_str!("../migrations/20261019000200_owed_push_operations.sql"),
    "\n",
    include_str!("../migrations/20261023000100_remote_wake_requests.sql"),
);

pub(super) struct TableSpec {
    pub(super) name: &'static str,
    pub(super) columns: &'static str,
    pub(super) foreign_keys: &'static str,
}

pub(super) const TABLE_SPECS: [TableSpec; 12] = [
    TableSpec {
        name: "automation_events",
        columns: "event_sequence,INTEGER,0,<NULL>,1;event_id,TEXT,1,<NULL>,0;subject_kind,TEXT,1,<NULL>,0;subject_id,TEXT,1,<NULL>,0;event_kind,TEXT,1,<NULL>,0;event_body_json,TEXT,1,<NULL>,0;recorded_at_ms,INTEGER,1,<NULL>,0",
        foreign_keys: "",
    },
    TableSpec {
        name: "instruction_documents",
        columns: "instruction_id,TEXT,0,<NULL>,1;current_revision_id,TEXT,1,<NULL>,0;instruction_text,TEXT,1,<NULL>,0;updated_at_ms,INTEGER,1,<NULL>,0",
        foreign_keys: "0,0,instruction_revisions,instruction_id,instruction_id,NO ACTION,NO ACTION,NONE;0,1,instruction_revisions,current_revision_id,revision_id,NO ACTION,NO ACTION,NONE",
    },
    TableSpec {
        name: "instruction_revisions",
        columns: "revision_id,TEXT,0,<NULL>,1;instruction_id,TEXT,1,<NULL>,0;instruction_text,TEXT,1,<NULL>,0;source_revision_id,TEXT,0,<NULL>,0;recorded_at_ms,INTEGER,1,<NULL>,0",
        foreign_keys: "0,0,instruction_documents,instruction_id,instruction_id,NO ACTION,NO ACTION,NONE",
    },
    TableSpec {
        name: "mailbox_deliveries",
        columns: "delivery_id,TEXT,0,<NULL>,1;wakeup_id,TEXT,1,<NULL>,0;occurrence_id,TEXT,1,<NULL>,0;due_at_ms,INTEGER,1,<NULL>,0;fired_at_ms,INTEGER,1,<NULL>,0;target_json,TEXT,1,<NULL>,0;message_json,TEXT,1,<NULL>,0;delivery_mode,TEXT,1,<NULL>,0;generation_guard_json,TEXT,0,<NULL>,0;delivery_status,TEXT,1,<NULL>,0;eligible_at_ms,INTEGER,1,<NULL>,0;expires_at_ms,INTEGER,0,<NULL>,0;latest_attempt_json,TEXT,0,<NULL>,0;accepted_receipt_json,TEXT,0,<NULL>,0;created_at_ms,INTEGER,1,<NULL>,0",
        foreign_keys: "0,0,wakeup_definitions,wakeup_id,wakeup_id,NO ACTION,NO ACTION,NONE",
    },
    TableSpec {
        name: "operation_receipts",
        columns: "operation_id,TEXT,0,<NULL>,1;method_name,TEXT,1,<NULL>,0;canonical_request,BLOB,1,<NULL>,0;resource_id,TEXT,1,<NULL>,0;operation_status,TEXT,1,<NULL>,0;effect_evidence_json,TEXT,1,<NULL>,0;final_result_json,TEXT,0,<NULL>,0;final_error_json,TEXT,0,<NULL>,0;committed_at_ms,INTEGER,1,<NULL>,0",
        foreign_keys: "",
    },
    TableSpec {
        name: "owed_push_operations",
        columns: "operation_id,TEXT,1,<NULL>,1;push_id,TEXT,1,<NULL>,0;operation,TEXT,1,<NULL>,0;owed_at,TEXT,1,<NULL>,0",
        foreign_keys: "",
    },
    TableSpec {
        name: "remote_wake_requests",
        columns: "operation_id,TEXT,1,<NULL>,1;target_json,TEXT,1,<NULL>,0;canonical_request,BLOB,1,<NULL>,0;body_json,TEXT,1,<NULL>,0;valid_until_ms,INTEGER,1,<NULL>,0;wakeup_id,TEXT,0,<NULL>,0;snapshot_json,TEXT,0,<NULL>,0;created_at_ms,INTEGER,0,<NULL>,0",
        foreign_keys: "",
    },
    TableSpec {
        name: "schedule_definitions",
        columns: "schedule_id,TEXT,0,<NULL>,1;change_id,TEXT,1,<NULL>,0;instruction_id,TEXT,1,<NULL>,0;enabled,INTEGER,1,<NULL>,0;definition_json,TEXT,1,<NULL>,0;imported_continuity_json,TEXT,1,<NULL>,0;created_at_ms,INTEGER,1,<NULL>,0;updated_at_ms,INTEGER,1,<NULL>,0",
        foreign_keys: "0,0,instruction_documents,instruction_id,instruction_id,NO ACTION,NO ACTION,NONE",
    },
    TableSpec {
        name: "schedule_timing_state",
        columns: "schedule_id,TEXT,0,<NULL>,1;applied_change_id,TEXT,1,<NULL>,0;anchor_at_ms,INTEGER,1,<NULL>,0;evaluated_through_ms,INTEGER,1,<NULL>,0;next_due_at_ms,INTEGER,0,<NULL>,0",
        foreign_keys: "0,0,schedule_definitions,schedule_id,schedule_id,NO ACTION,NO ACTION,NONE",
    },
    TableSpec {
        name: "thread_bindings",
        columns: "thread_binding_id,TEXT,0,<NULL>,1;schedule_id,TEXT,1,<NULL>,0;service_id,TEXT,1,<NULL>,0;endpoint_id,TEXT,1,<NULL>,0;thread_id,TEXT,1,<NULL>,0;claimed_at_ms,INTEGER,1,<NULL>,0",
        foreign_keys: "0,0,schedule_definitions,schedule_id,schedule_id,NO ACTION,NO ACTION,NONE",
    },
    TableSpec {
        name: "wakeup_definitions",
        columns: "wakeup_id,TEXT,0,<NULL>,1;change_id,TEXT,1,<NULL>,0;wakeup_status,TEXT,1,<NULL>,0;definition_json,TEXT,1,<NULL>,0;anchor_at_ms,INTEGER,1,<NULL>,0;evaluated_through_ms,INTEGER,1,<NULL>,0;next_due_at_ms,INTEGER,0,<NULL>,0;expires_at_ms,INTEGER,0,<NULL>,0;first_fire_json,TEXT,0,<NULL>,0;pending_delivery_id,TEXT,0,<NULL>,0;created_at_ms,INTEGER,1,<NULL>,0;updated_at_ms,INTEGER,1,<NULL>,0",
        foreign_keys: "0,0,mailbox_deliveries,wakeup_id,wakeup_id,NO ACTION,NO ACTION,NONE;0,1,mailbox_deliveries,pending_delivery_id,delivery_id,NO ACTION,NO ACTION,NONE",
    },
    TableSpec {
        name: "workflow_runs",
        columns: "run_id,TEXT,0,<NULL>,1;schedule_id,TEXT,1,<NULL>,0;due_at_ms,INTEGER,1,<NULL>,0;run_status,TEXT,1,<NULL>,0;captured_inputs_json,TEXT,0,<NULL>,0;thread_binding_id,TEXT,0,<NULL>,0;native_turn_id,TEXT,0,<NULL>,0;execution_started_at_ms,INTEGER,0,<NULL>,0;execution_deadline_at_ms,INTEGER,0,<NULL>,0;effective_timeout_seconds,INTEGER,0,<NULL>,0;execution_evidence_json,TEXT,1,<NULL>,0;worker_outcome_json,TEXT,0,<NULL>,0;summary_attempt_json,TEXT,0,<NULL>,0;summary_text,TEXT,0,<NULL>,0;summary_source_json,TEXT,0,<NULL>,0;completed_at_ms,INTEGER,0,<NULL>,0",
        foreign_keys: "0,0,thread_bindings,schedule_id,schedule_id,NO ACTION,NO ACTION,NONE;0,1,thread_bindings,thread_binding_id,thread_binding_id,NO ACTION,NO ACTION,NONE;1,0,schedule_definitions,schedule_id,schedule_id,NO ACTION,NO ACTION,NONE",
    },
];

pub(super) const INDEX_SPECS: [&str; 27] = [
    "automation_events,event_cleanup,0,c,0,recorded_at_ms:0:BINARY:1,event_sequence:0:BINARY:1",
    "automation_events,event_history,0,c,0,subject_kind:0:BINARY:1,subject_id:0:BINARY:1,event_sequence:0:BINARY:1",
    "automation_events,_,1,u,0,event_id:0:BINARY:1",
    "instruction_documents,_,1,pk,0,instruction_id:0:BINARY:1",
    "instruction_revisions,_,1,u,0,instruction_id:0:BINARY:1,revision_id:0:BINARY:1",
    "instruction_revisions,_,1,pk,0,revision_id:0:BINARY:1",
    "mailbox_deliveries,delivery_eligibility,0,c,0,delivery_status:0:BINARY:1,eligible_at_ms:0:BINARY:1",
    "mailbox_deliveries,_,1,u,0,wakeup_id:0:BINARY:1,delivery_id:0:BINARY:1",
    "mailbox_deliveries,_,1,u,0,occurrence_id:0:BINARY:1",
    "mailbox_deliveries,_,1,pk,0,delivery_id:0:BINARY:1",
    "operation_receipts,_,1,pk,0,operation_id:0:BINARY:1",
    "owed_push_operations,owed_push_operations_order,0,c,0,push_id:0:BINARY:1,operation_id:0:BINARY:1",
    "owed_push_operations,_,1,pk,0,operation_id:0:BINARY:1",
    "remote_wake_requests,_,1,pk,0,operation_id:0:BINARY:1",
    "remote_wake_requests,_,1,u,0,wakeup_id:0:BINARY:1",
    "schedule_definitions,_,1,pk,0,schedule_id:0:BINARY:1",
    "schedule_timing_state,_,1,pk,0,schedule_id:0:BINARY:1",
    "thread_bindings,_,1,u,0,schedule_id:0:BINARY:1,thread_binding_id:0:BINARY:1",
    "thread_bindings,_,1,u,0,service_id:0:BINARY:1,endpoint_id:0:BINARY:1,thread_id:0:BINARY:1",
    "thread_bindings,_,1,pk,0,thread_binding_id:0:BINARY:1",
    "wakeup_definitions,wakeup_occupying_target,1,c,1,<EXPRESSION>:0:BINARY:1,<EXPRESSION>:0:BINARY:1,<EXPRESSION>:0:BINARY:1",
    "wakeup_definitions,_,1,u,0,pending_delivery_id:0:BINARY:1",
    "wakeup_definitions,_,1,pk,0,wakeup_id:0:BINARY:1",
    "workflow_runs,run_history,0,c,0,schedule_id:0:BINARY:1,due_at_ms:0:BINARY:1,run_id:0:BINARY:1",
    "workflow_runs,run_admission_lookup,0,c,0,schedule_id:0:BINARY:1,run_status:0:BINARY:1",
    "workflow_runs,_,1,u,0,schedule_id:0:BINARY:1,run_id:0:BINARY:1",
    "workflow_runs,_,1,pk,0,run_id:0:BINARY:1",
];
