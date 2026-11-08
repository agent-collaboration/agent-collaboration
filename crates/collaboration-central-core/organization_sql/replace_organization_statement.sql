UPDATE organization_identity
SET organization_statement = ?, updated_at = CAST(? AS INTEGER)
WHERE id = ? AND organization_statement = ?;
