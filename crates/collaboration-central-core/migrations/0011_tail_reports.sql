CREATE TABLE IF NOT EXISTS tail_reports (project TEXT NOT NULL, epoch TEXT NOT NULL, request_ids TEXT NOT NULL, digest TEXT NOT NULL, PRIMARY KEY(project,epoch));
