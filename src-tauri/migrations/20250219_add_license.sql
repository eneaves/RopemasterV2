CREATE TABLE IF NOT EXISTS license (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    raw_bytes BLOB NOT NULL,
    installed_at INTEGER NOT NULL,
    last_verified_at INTEGER NOT NULL
);
