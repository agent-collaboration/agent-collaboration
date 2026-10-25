SELECT request_id, canonical_hash, payload, router_time, time_basis, workspace_id, board_id, origin_installation, origin_machine
FROM fleet_request_decisions
WHERE request_id = ?;
