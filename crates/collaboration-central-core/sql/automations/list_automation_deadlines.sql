SELECT CAST(deadline_ms AS TEXT) AS deadline_ms FROM (
SELECT t.next_due_ms AS deadline_ms
FROM automation_timing t JOIN automations a USING(automation_id)
WHERE a.state IN ('enabled','active') AND a.fault_json IS NULL AND t.next_due_ms IS NOT NULL
UNION ALL
SELECT t.expires_at_ms AS deadline_ms
FROM automation_timing t JOIN automations a USING(automation_id)
WHERE a.kind='wake' AND a.state IN ('active','paused') AND t.expires_at_ms IS NOT NULL
);
