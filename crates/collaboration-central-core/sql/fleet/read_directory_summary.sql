SELECT CAST(project_id AS TEXT) AS project_id,typeof(project_id) AS project_id_class,
CAST(copy_epoch AS TEXT) AS copy_epoch,
typeof(copy_epoch) AS copy_epoch_class,
CAST(copy_sequence AS TEXT) AS copy_sequence,
typeof(copy_sequence) AS copy_sequence_class,
CAST(copy_acked_at AS TEXT) AS copy_acked_at,
typeof(copy_acked_at) AS copy_acked_at_class,
CAST(copy_latest_time_basis AS TEXT) AS copy_latest_time_basis,
typeof(copy_latest_time_basis) AS copy_latest_time_basis_class,
CAST(copy_latest_record_at AS TEXT) AS copy_latest_record_at,
typeof(copy_latest_record_at) AS copy_latest_record_at_class
FROM fleet_project_directory WHERE project_id=?;
