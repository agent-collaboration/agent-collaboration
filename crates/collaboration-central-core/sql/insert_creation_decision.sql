INSERT INTO fleet_creation_decisions (project_id, creating_request, carrier_installation, created_at, updated_at)
VALUES (?1, ?2, ?3, CAST(?4 AS INTEGER), CAST(?4 AS INTEGER));
