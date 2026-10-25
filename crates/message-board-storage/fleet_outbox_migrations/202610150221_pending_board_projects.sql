CREATE INDEX pending_project_boards ON fleet_outbox_requests(state, json_extract(request, '$.operation.kind'), json_extract(request, '$.operation.board'), request_id);
