SELECT CAST(id AS TEXT) AS id, typeof(id) AS id_class,
    CAST(installation_id AS TEXT) AS installation_id, typeof(installation_id) AS installation_id_class,
    CAST(workspace_id AS TEXT) AS workspace_id, typeof(workspace_id) AS workspace_id_class,
    CAST(state AS TEXT) AS state, typeof(state) AS state_class
FROM machine_allowances WHERE installation_id = ? ORDER BY id;
