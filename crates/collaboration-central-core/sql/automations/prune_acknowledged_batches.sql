DELETE FROM automation_batches WHERE batch_id IN (
    SELECT batch_id FROM automation_batches
    WHERE acked_at_ms IS NOT NULL AND acked_at_ms < CAST(? AS INTEGER)
    ORDER BY acked_at_ms, batch_id LIMIT 1000
);
