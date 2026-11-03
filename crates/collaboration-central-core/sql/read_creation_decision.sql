SELECT c.project_id, c.creating_request, c.carrier_installation,
       CAST(c.created_at AS TEXT) AS created_at, typeof(c.created_at) AS created_class,
       CAST(c.updated_at AS TEXT) AS updated_at, typeof(c.updated_at) AS updated_class,
       d.request_id, d.canonical_hash, d.payload, d.router_time, d.time_basis,
       d.workspace_id, d.board_id, d.origin_installation, d.origin_machine
FROM fleet_creation_decisions AS c
LEFT JOIN fleet_request_decisions AS d ON d.request_id = c.creating_request
WHERE c.project_id = ?1;
