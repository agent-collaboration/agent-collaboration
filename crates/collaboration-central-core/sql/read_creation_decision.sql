SELECT c.project_id, c.creating_request, c.carrier_installation,
       d.request_id, d.canonical_hash, d.payload, d.router_time, d.time_basis,
       d.workspace_id, d.board_id, d.origin_installation, d.origin_machine
FROM creation_decisions AS c
LEFT JOIN fleet_request_decisions AS d ON d.request_id = c.creating_request
WHERE c.project_id = ?1;
