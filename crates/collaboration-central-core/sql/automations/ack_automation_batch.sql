UPDATE automation_batches SET acked_at_ms=COALESCE(acked_at_ms,CAST(? AS INTEGER)) WHERE batch_id=?;
