SELECT CAST(request_id AS TEXT) AS request_id,
    CAST(workspace_id AS TEXT) AS workspace_id,
    CAST(decision_kind AS TEXT) AS decision_kind,
    CAST(change_kind AS TEXT) AS change_kind,
    CAST(origin_installation AS TEXT) AS origin_installation,
    CAST(origin_machine AS TEXT) AS origin_machine
FROM fleet_request_decisions ORDER BY request_id;
