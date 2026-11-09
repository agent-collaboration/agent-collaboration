INSERT INTO machine_installations(
    id, former_service_id, label, enrollment_generation, last_seen_at, created_at, updated_at
) VALUES(?, ?, ?, 0, CAST(? AS INTEGER), CAST(? AS INTEGER), CAST(? AS INTEGER));
