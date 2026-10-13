CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    -- bcrypt hash of the client-sent key (md5(password)) combined with the
    -- server-side salt; the key itself is never stored
    password_hash TEXT NOT NULL,
    -- unix nanoseconds, stamped by the server (SQLite's own clock stops at millis)
    created_at_ns INTEGER NOT NULL
) STRICT;

-- Append-only: every push adds a row, nothing is updated in place. Rows are
-- ordered by id, which follows insertion order even if the clock steps back.
CREATE TABLE progress_history (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    document TEXT NOT NULL,
    progress TEXT NOT NULL,
    percentage REAL NOT NULL,
    device TEXT NOT NULL,
    device_id TEXT,
    filename TEXT,
    title TEXT,
    authors TEXT,
    -- server clock at write time, unix nanoseconds
    timestamp_ns INTEGER NOT NULL
) STRICT;

CREATE INDEX progress_history_document ON progress_history (user_id, document, id);

CREATE INDEX progress_history_device ON progress_history (user_id, document, device_id, id);

-- Most recent entry for each (user_id, document)
CREATE VIEW latest_progress AS
SELECT p.*
FROM progress_history AS p
WHERE p.id = (
    SELECT max(h.id)
    FROM progress_history AS h
    WHERE h.user_id = p.user_id AND h.document = p.document
);

-- Most recent entry for each (user_id, document, device_id). Entries without
-- a device_id form one group per document.
CREATE VIEW latest_device_progress AS
SELECT p.*
FROM progress_history AS p
WHERE p.id = (
    SELECT max(h.id)
    FROM progress_history AS h
    WHERE h.user_id = p.user_id AND h.document = p.document AND h.device_id IS p.device_id
);
