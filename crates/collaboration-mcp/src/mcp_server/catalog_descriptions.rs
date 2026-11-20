pub(super) fn operation_description(name: &str) -> &'static str {
    match name {
        "conversation_operation_show" => {
            "Reads durable metadata for one conversation operation by its caller-retained operationId. Read-only; it never dispatches, waits for or replays client work."
        }
        "conversation_operation_wait" => {
            "Waits boundedly for settlement of an already admitted conversation operation. The waiter is call-local: MCP cancellation detaches it without cancelling client work or replaying the operation."
        }
        "conversation_operation_reconcile" => {
            "Reconciles one conversation operation from exact supported evidence using its caller-retained operationId. It is read-only with respect to client work and never resubmits the operation."
        }
        _ => {
            "Undocumented collaboration operation; inspect the typed input and output schemas before calling."
        }
    }
}
