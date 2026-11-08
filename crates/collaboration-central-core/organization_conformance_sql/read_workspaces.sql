SELECT CAST(workspace_id AS TEXT) AS workspace_id,
    CAST(name AS TEXT) AS name,
    CAST(state AS TEXT) AS state,
    CAST(request_id AS TEXT) AS request_id,
    CAST(origin_installation AS TEXT) AS origin_installation,
    CAST(origin_machine AS TEXT) AS origin_machine
FROM fleet_workspaces ORDER BY workspace_id;
