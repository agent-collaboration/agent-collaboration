SELECT service_id, peer_id, organization_id, claimed_at
FROM fleet_machines
WHERE service_id = ? OR peer_id = ?
ORDER BY service_id;
