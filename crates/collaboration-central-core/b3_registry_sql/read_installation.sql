SELECT CAST(id AS TEXT) AS id, typeof(id) AS id_class,
    CAST(former_service_id AS TEXT) AS former_service_id, typeof(former_service_id) AS former_service_id_class,
    CAST(label AS TEXT) AS label, typeof(label) AS label_class,
    CAST(enrollment_generation AS TEXT) AS enrollment_generation, typeof(enrollment_generation) AS enrollment_generation_class
FROM machine_installations WHERE id = ?;
