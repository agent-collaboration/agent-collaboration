CREATE INDEX cached_project_boards ON cached_fleet_records(json_extract(payload, '$.record.board'), id) WHERE kind = 'project';
