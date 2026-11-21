SELECT caller, answer
FROM workspace_request_log
WHERE request_id = ? AND created_at > ?
