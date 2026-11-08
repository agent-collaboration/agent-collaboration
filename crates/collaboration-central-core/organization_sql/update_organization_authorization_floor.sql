UPDATE organization_authorization_floors
SET minimum_revision = ?2, state = ?3, permission_policy_digest = ?4, updated_at = ?5
WHERE id = ?1;
