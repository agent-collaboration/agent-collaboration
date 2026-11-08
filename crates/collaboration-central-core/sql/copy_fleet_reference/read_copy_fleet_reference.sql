SELECT CAST(project_id AS TEXT) AS project_id,
    typeof(project_id) AS project_id_class,
    CAST(fleet_address_key AS TEXT) AS fleet_address_key,
    typeof(fleet_address_key) AS fleet_address_key_class
FROM _copy_fleet_reference WHERE project_id = ?;
