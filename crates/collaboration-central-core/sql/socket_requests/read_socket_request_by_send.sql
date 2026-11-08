SELECT subject,kind,connection_id,CAST(sent_version AS TEXT) AS sent_version,typeof(sent_version) AS sent_version_class FROM central_work_attempts WHERE send_id=?1;
