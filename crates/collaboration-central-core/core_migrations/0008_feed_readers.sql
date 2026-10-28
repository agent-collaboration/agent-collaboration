CREATE TABLE feed_readers (
    reader_id TEXT NOT NULL PRIMARY KEY,
    installation TEXT NOT NULL UNIQUE,
    last_read_position INTEGER NOT NULL
);
