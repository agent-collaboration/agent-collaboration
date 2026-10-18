CREATE TABLE IF NOT EXISTS archive_reports (board TEXT NOT NULL, project TEXT NOT NULL, position_epoch TEXT NOT NULL, position_commit TEXT NOT NULL, installation TEXT NOT NULL, reported_at TEXT NOT NULL, PRIMARY KEY(board,project));
CREATE TABLE IF NOT EXISTS claim_reservations (project TEXT NOT NULL, attempt TEXT NOT NULL, record TEXT NOT NULL, PRIMARY KEY(project,attempt));
