SELECT reader_id, installation, CAST(last_read_position AS TEXT) AS last_read_position, typeof(last_read_position) AS position_class FROM feed_readers ORDER BY installation;
