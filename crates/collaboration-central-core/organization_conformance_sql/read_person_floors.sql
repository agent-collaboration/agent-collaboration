SELECT CAST(id AS TEXT) AS subject_id,
    CAST(person_id AS TEXT) AS person_id,
    CAST(minimum_revision AS TEXT) AS minimum_revision,
    CAST(state AS TEXT) AS state
FROM person_organization_authorization_floors WHERE organization_id = ? ORDER BY person_id;
