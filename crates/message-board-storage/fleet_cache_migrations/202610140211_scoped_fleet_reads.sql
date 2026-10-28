CREATE INDEX cached_fleet_by_workspace ON cached_fleet_records(kind, json_extract(payload, '$.record.envelope.scope.workspace'), id);
