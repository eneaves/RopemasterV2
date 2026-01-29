-- Enable foreign keys
PRAGMA foreign_keys = ON;

-- ========================
-- TABLAS DE IDENTIDAD / AUTH
-- ========================
CREATE TABLE app_user (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  email         TEXT NOT NULL UNIQUE,
  full_name     TEXT NOT NULL,
  password_hash TEXT NOT NULL,
  is_active     INTEGER NOT NULL DEFAULT 1,
  created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);

CREATE TABLE role (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  name       TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);

CREATE TABLE user_role (
  user_id  INTEGER NOT NULL REFERENCES app_user(id) ON DELETE CASCADE,
  role_id  INTEGER NOT NULL REFERENCES role(id)     ON DELETE CASCADE,
  PRIMARY KEY (user_id, role_id)
);

CREATE TABLE audit_log (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id      INTEGER REFERENCES app_user(id) ON DELETE SET NULL,
  action       TEXT NOT NULL,
  entity_type  TEXT NOT NULL,
  entity_id    INTEGER,
  metadata     TEXT,
  created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE INDEX idx_audit_entity ON audit_log(entity_type, entity_id);

-- ========================
-- LICENCIAS / ACTIVIDAD
-- ========================
CREATE TABLE license_info (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  customer     TEXT NOT NULL,
  device_id    TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  signature_b64 TEXT NOT NULL,
  valid_from   TEXT,
  valid_until  TEXT,
  is_valid     INTEGER NOT NULL DEFAULT 0,
  created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);

CREATE TABLE activity (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id    INTEGER REFERENCES app_user(id) ON DELETE SET NULL,
  kind       TEXT NOT NULL,
  details    TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE INDEX idx_activity_kind_time ON activity(kind, created_at);

-- ========================
-- SERIES / EVENTOS / ROPERS / TEAMS
-- ========================
CREATE TABLE series (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  name           TEXT NOT NULL,
  season         TEXT NOT NULL,
  status         TEXT NOT NULL CHECK (status IN ('active','upcoming','archived')),
  start_date     TEXT,
  end_date       TEXT,
  is_deleted     INTEGER NOT NULL DEFAULT 0,
  created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  UNIQUE(name, season)
);

CREATE TABLE event (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  series_id    INTEGER NOT NULL REFERENCES series(id) ON DELETE CASCADE,
  name         TEXT NOT NULL,
  date         TEXT NOT NULL,
  status       TEXT NOT NULL CHECK (status IN ('active','upcoming','completed','locked')) DEFAULT 'upcoming',
  rounds       INTEGER NOT NULL CHECK (rounds >= 1 AND rounds <= 10),
  location     TEXT,
  entry_fee    REAL,
  prize_pool   REAL,
  is_deleted   INTEGER NOT NULL DEFAULT 0,
  created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE INDEX idx_event_series ON event(series_id);
CREATE UNIQUE INDEX uq_event_series_name_date ON event(series_id, name, date);

CREATE TABLE roper (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  first_name   TEXT NOT NULL,
  last_name    TEXT NOT NULL,
  specialty    TEXT NOT NULL CHECK (specialty IN ('header','heeler','both')),
  rating       REAL NOT NULL DEFAULT 0.0,
  phone        TEXT,
  email        TEXT,
  created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE INDEX idx_roper_name ON roper(last_name, first_name);

CREATE TABLE team (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  event_id     INTEGER NOT NULL REFERENCES event(id) ON DELETE CASCADE,
  header_id    INTEGER NOT NULL REFERENCES roper(id) ON DELETE RESTRICT,
  heeler_id    INTEGER NOT NULL REFERENCES roper(id) ON DELETE RESTRICT,
  rating       REAL NOT NULL DEFAULT 0.0,
  status       TEXT NOT NULL CHECK (status IN ('active','inactive')) DEFAULT 'active',
  created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  UNIQUE(event_id, header_id, heeler_id)
);
CREATE INDEX idx_team_event ON team(event_id);

-- ========================
-- DRAW / RUNS / STANDINGS / PAYOFFS
-- ========================
CREATE TABLE draw (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  event_id   INTEGER NOT NULL REFERENCES event(id) ON DELETE CASCADE,
  round      INTEGER NOT NULL CHECK (round >= 1),
  position   INTEGER NOT NULL CHECK (position >= 1),
  team_id    INTEGER NOT NULL REFERENCES team(id) ON DELETE CASCADE,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  UNIQUE(event_id, round, position)
);
CREATE INDEX idx_draw_event_round ON draw(event_id, round);

CREATE TABLE run (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  event_id   INTEGER NOT NULL REFERENCES event(id) ON DELETE CASCADE,
  team_id    INTEGER NOT NULL REFERENCES team(id) ON DELETE CASCADE,
  round      INTEGER NOT NULL CHECK (round >= 1),
  position   INTEGER NOT NULL,
  time_sec   REAL,
  penalty    REAL NOT NULL DEFAULT 0.0,
  total_sec  REAL,
  no_time    INTEGER NOT NULL DEFAULT 0,
  dq         INTEGER NOT NULL DEFAULT 0,
  status     TEXT NOT NULL CHECK (status IN ('pending','completed','skipped')) DEFAULT 'pending',
  captured_by INTEGER REFERENCES app_user(id) ON DELETE SET NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  UNIQUE(event_id, round, team_id)
);
CREATE INDEX idx_run_event_round ON run(event_id, round);
CREATE INDEX idx_run_event_pos   ON run(event_id, round, position);

CREATE TABLE payoff_rule (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  event_id    INTEGER NOT NULL REFERENCES event(id) ON DELETE CASCADE,
  position    INTEGER NOT NULL CHECK (position >= 1),
  percentage  REAL NOT NULL CHECK (percentage >= 0.0 AND percentage <= 1.0),
  created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  UNIQUE(event_id, position)
);

CREATE TABLE payoff (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  event_id    INTEGER NOT NULL REFERENCES event(id) ON DELETE CASCADE,
  team_id     INTEGER NOT NULL REFERENCES team(id) ON DELETE CASCADE,
  position    INTEGER NOT NULL CHECK (position >= 1),
  total_time  REAL NOT NULL,
  amount      REAL NOT NULL,
  created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  UNIQUE(event_id, position),
  UNIQUE(event_id, team_id)
);
CREATE INDEX idx_payoff_event ON payoff(event_id);

-- ========================
-- SEEDS BÁSICOS
-- ========================
INSERT INTO role (name) VALUES ('admin') ON CONFLICT DO NOTHING;
INSERT INTO role (name) VALUES ('operator') ON CONFLICT DO NOTHING;
INSERT INTO role (name) VALUES ('viewer') ON CONFLICT DO NOTHING;
