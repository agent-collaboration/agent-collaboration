SELECT CAST(machine_id AS TEXT) AS machine_id, typeof(machine_id) AS machine_id_class,
    CAST(service_id AS TEXT) AS service_id, typeof(service_id) AS service_id_class,
    CAST(machine_label AS TEXT) AS machine_label, typeof(machine_label) AS machine_label_class,
    CAST(last_seen_at AS TEXT) AS last_seen_at, typeof(last_seen_at) AS last_seen_at_class,
    CAST(created_at AS TEXT) AS created_at, typeof(created_at) AS created_at_class
FROM machine_enrollments ORDER BY machine_id;
