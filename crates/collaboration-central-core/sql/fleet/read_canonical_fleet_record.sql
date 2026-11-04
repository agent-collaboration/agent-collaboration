-- Workerd permits five compound terms; scripts/tooling/measure-central-workerd-sql.mjs checks this boundary.
SELECT * FROM (
SELECT CAST('project' AS TEXT) AS kind, project_id AS record_id, workspace_id AS workspace_id, board_id AS board_id, name AS business_name, payload AS payload, router_time AS router_time, time_basis AS time_basis, origin_installation AS origin_installation, origin_machine AS origin_machine, import_digest AS import_digest FROM project_directory
UNION ALL
SELECT CAST('adr' AS TEXT) AS kind, adr_id AS record_id, workspace_id AS workspace_id, CAST(NULL AS TEXT) AS board_id, title AS business_name, payload AS payload, router_time AS router_time, time_basis AS time_basis, origin_installation AS origin_installation, origin_machine AS origin_machine, import_digest AS import_digest FROM workspace_adrs
) WHERE kind = ? AND record_id = ?;
