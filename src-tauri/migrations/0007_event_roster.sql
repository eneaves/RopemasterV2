-- 0005_event_roster.sql
-- Introduce un roster por evento y normaliza el directorio global de ropers.

-- =========
-- Nuevas columnas para roper (metadatos y normalización)
-- =========
ALTER TABLE roper ADD COLUMN external_id TEXT;
ALTER TABLE roper ADD COLUMN normalized_phone TEXT;
ALTER TABLE roper ADD COLUMN country_code TEXT;
ALTER TABLE roper ADD COLUMN default_event_level TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_roper_external_id ON roper(external_id) WHERE external_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_roper_normalized_phone ON roper(normalized_phone) WHERE normalized_phone IS NOT NULL;

-- =========
-- Tabla event_roster
-- =========
CREATE TABLE event_roster (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  event_id        INTEGER NOT NULL REFERENCES event(id) ON DELETE CASCADE,
  roper_id        INTEGER NOT NULL REFERENCES roper(id) ON DELETE CASCADE,
  status          TEXT NOT NULL CHECK (status IN ('registered','confirmed','withdrawn')) DEFAULT 'registered',
  rating_override REAL,
  source_hash     TEXT,
  notes           TEXT,
  created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  UNIQUE(event_id, roper_id)
);

CREATE INDEX idx_event_roster_event ON event_roster(event_id);
CREATE INDEX idx_event_roster_status ON event_roster(event_id, status);

-- =========
-- Poblar roster inicial con los ropers que ya tienen equipos
-- =========
INSERT OR IGNORE INTO event_roster (event_id, roper_id, status, created_at, updated_at)
SELECT DISTINCT
  event_id,
  header_id,
  'registered',
  COALESCE(created_at, strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  COALESCE(updated_at, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
FROM team
WHERE header_id IS NOT NULL;

INSERT OR IGNORE INTO event_roster (event_id, roper_id, status, created_at, updated_at)
SELECT DISTINCT
  event_id,
  heeler_id,
  'registered',
  COALESCE(created_at, strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  COALESCE(updated_at, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
FROM team
WHERE heeler_id IS NOT NULL;
