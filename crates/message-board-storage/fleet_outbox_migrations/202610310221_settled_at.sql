ALTER TABLE fleet_outbox_requests ADD COLUMN settled_at_ms INTEGER;
UPDATE fleet_outbox_requests
   SET settled_at_ms=CAST(strftime('%s','now') AS INTEGER)*1000
 WHERE json_extract(state,'$.kind')='decided';
CREATE INDEX fleet_outbox_settled_at ON fleet_outbox_requests(settled_at_ms,request_id);
