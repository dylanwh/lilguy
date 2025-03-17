
CREATE TABLE IF NOT EXISTS lg_internal (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    value TEXT NOT NULL
);

-- session is just a json blob store (using JSONB)
CREATE TABLE IF NOT EXISTS lg_session (
    uuid TEXT PRIMARY KEY,
    data JSONB NOT NULL
)

