DELETE FROM fleet_repository_attachments WHERE workspace_id=?1 AND reference_kind=?2 AND normalized_origin IS ?3 AND service_id IS ?4 AND common_directory IS ?5;
