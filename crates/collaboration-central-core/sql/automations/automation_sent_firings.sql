SELECT firing_id FROM automation_firings WHERE automation_id=? AND batch_id IS NOT NULL ORDER BY due_ms,firing_id;
