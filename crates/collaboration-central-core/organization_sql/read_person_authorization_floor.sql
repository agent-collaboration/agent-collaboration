SELECT CAST(id AS TEXT) AS id,
    typeof(id) AS id_class,
    CAST(organization_id AS TEXT) AS organization_id,
    typeof(organization_id) AS organization_id_class,
    CAST(person_id AS TEXT) AS person_id,
    typeof(person_id) AS person_id_class,
    CAST(minimum_revision AS TEXT) AS minimum_revision,
    typeof(minimum_revision) AS minimum_revision_class,
    CAST(state AS TEXT) AS state,
    typeof(state) AS state_class
FROM person_organization_authorization_floors
WHERE organization_id = ? AND person_id = ?;
