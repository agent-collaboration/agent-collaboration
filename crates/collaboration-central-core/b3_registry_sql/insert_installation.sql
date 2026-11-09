INSERT INTO machine_installations(
    id, former_service_id, label, enrollment_generation, last_seen_at, created_at, updated_at
) VALUES(?, NULL, ?, CAST(? AS INTEGER), NULL, CAST(? AS INTEGER), CAST(? AS INTEGER));
