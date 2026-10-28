SELECT p.push_id AS "push_id!: String"
FROM router_pushes p
WHERE target_service_id = ? AND target_endpoint_id = ? AND target_session_id = ?
  AND retention = 'mailTerminalAge' AND delivery_state = 'held'
  AND NOT EXISTS (
    SELECT 1 FROM local_delivery_retries r
    WHERE r.item_id = p.push_id AND r.scheduled_retry_at IS NOT NULL
  )
  AND NOT EXISTS (SELECT 1 FROM pending_poll_mail poll WHERE poll.item_id=p.push_id)
ORDER BY created_at, push_id
