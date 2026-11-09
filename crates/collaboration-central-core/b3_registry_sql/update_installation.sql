UPDATE machine_installations
SET label = ?, enrollment_generation = CAST(? AS INTEGER), updated_at = CAST(? AS INTEGER)
WHERE id = ?;
