UPDATE machine_installations
SET former_service_id = ?, updated_at = CAST(? AS INTEGER)
WHERE id = ? AND former_service_id IS NULL;
