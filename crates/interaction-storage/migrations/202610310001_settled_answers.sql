ALTER TABLE message_send_answers ADD COLUMN settled_at_ms INTEGER;
UPDATE message_send_answers
SET settled_at_ms=CAST(strftime('%s','now') AS INTEGER)*1000
WHERE result IS NOT NULL;
CREATE INDEX message_send_answers_settled ON message_send_answers(settled_at_ms,item_id);

ALTER TABLE local_cancel_deliveries ADD COLUMN settled_at_ms INTEGER;
UPDATE local_cancel_deliveries
SET settled_at_ms=CAST(strftime('%s','now') AS INTEGER)*1000
WHERE outcome_json IS NOT NULL;
CREATE INDEX local_cancel_deliveries_settled ON local_cancel_deliveries(settled_at_ms,item_id);
