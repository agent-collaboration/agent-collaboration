DELETE FROM fleet_adr_repository_files WHERE adr_id=? AND ordinal>=CAST(? AS INTEGER);
