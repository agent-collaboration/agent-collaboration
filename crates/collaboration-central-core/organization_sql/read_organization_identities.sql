SELECT CAST(id AS TEXT) AS id,
    typeof(id) AS id_class,
    CAST(organization_statement AS TEXT) AS organization_statement,
    typeof(organization_statement) AS organization_statement_class
FROM organization_identity ORDER BY id LIMIT 2;
