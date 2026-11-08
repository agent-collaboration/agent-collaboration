UPDATE person_organization_authorization_floors
SET minimum_revision = ?2, state = ?3, updated_at = ?4
WHERE id = ?1;
