CREATE INDEX cached_fleet_by_board ON cached_fleet_records(kind, json_extract(payload, '$.record.envelope.scope.board'), id);
