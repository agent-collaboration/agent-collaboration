ALTER TABLE local_delivery_retries ADD COLUMN scheduled_retry_at TEXT;
ALTER TABLE router_pushes ADD COLUMN last_outcome_at TEXT;
