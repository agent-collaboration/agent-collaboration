SELECT CAST(id AS TEXT) AS id, typeof(id) AS id_class,
    CAST(installation_id AS TEXT) AS installation_id, typeof(installation_id) AS installation_id_class,
    CAST(request_id AS TEXT) AS request_id, typeof(request_id) AS request_id_class,
    CAST(state AS TEXT) AS state, typeof(state) AS state_class,
    CAST(end_reason AS TEXT) AS end_reason, typeof(end_reason) AS end_reason_class,
    CAST(approved_by_person_id AS TEXT) AS approved_by_person_id, typeof(approved_by_person_id) AS approved_by_person_id_class,
    CAST(permission_checked AS TEXT) AS permission_checked, typeof(permission_checked) AS permission_checked_class,
    CAST(base_generation AS TEXT) AS base_generation, typeof(base_generation) AS base_generation_class,
    CAST(code_expires_at AS TEXT) AS code_expires_at, typeof(code_expires_at) AS code_expires_at_class,
    CAST(machine_client_id AS TEXT) AS machine_client_id, typeof(machine_client_id) AS machine_client_id_class,
    CAST(last_grant_at AS TEXT) AS last_grant_at, typeof(last_grant_at) AS last_grant_at_class,
    CAST(ended_at AS TEXT) AS ended_at, typeof(ended_at) AS ended_at_class,
    CAST(created_at AS TEXT) AS created_at, typeof(created_at) AS created_at_class
FROM machine_enrollments WHERE installation_id = ? AND state = 'enrolled';
