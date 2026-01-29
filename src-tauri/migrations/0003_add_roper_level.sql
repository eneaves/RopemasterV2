-- 0003_add_roper_level.sql
-- Add column with default 'amateur'
ALTER TABLE roper ADD COLUMN level TEXT NOT NULL DEFAULT 'amateur';

-- Ensure any existing NULLs are set to 'amateur'
UPDATE roper SET level = 'amateur' WHERE level IS NULL;

-- Enforce allowed values via triggers (INSERT/UPDATE)
CREATE TRIGGER IF NOT EXISTS roper_level_check_insert
BEFORE INSERT ON roper
WHEN NEW.level NOT IN ('pro','amateur','principiante')
BEGIN
  SELECT RAISE(ABORT, "Nivel inválido: use 'pro', 'amateur' o 'principiante'.");
END;

CREATE TRIGGER IF NOT EXISTS roper_level_check_update
BEFORE UPDATE ON roper
WHEN NEW.level NOT IN ('pro','amateur','principiante')
BEGIN
  SELECT RAISE(ABORT, "Nivel inválido: use 'pro', 'amateur' o 'principiante'.");
END;
