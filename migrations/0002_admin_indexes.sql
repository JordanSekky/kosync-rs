-- One user's history across all of their documents, newest first
CREATE INDEX progress_history_user ON progress_history (user_id, id);

-- Time-range filters on the admin API
CREATE INDEX progress_history_timestamp ON progress_history (timestamp_ns);
