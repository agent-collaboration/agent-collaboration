CREATE INDEX pending_fleet_by_workspace ON fleet_outbox_requests(state, json_extract(request, '$.workspace'), json_extract(request, '$.operation.kind'), request_id);
