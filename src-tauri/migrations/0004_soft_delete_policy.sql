-- 0004_soft_delete_policy.sql
-- Asegura columnas y valores por defecto para la política de soft delete.

-- OBSERVACIÓN: algunas columnas ya se introdujeron en 0001_init.sql (p.ej. app_user.is_active,
-- series.is_deleted, event.is_deleted). Para evitar errores de "duplicate column name" en bases
-- nuevas, esta migración sólo añade las columnas que faltan respecto a 0001..0003.

-- Nota importante: SQLite no soporta ALTER TABLE ... ADD COLUMN IF NOT EXISTS. Por tanto esta
-- migración está escrita para alterarse lo menos posible y documentar su intención. Si tu base de
-- datos ya contiene las columnas mencionadas abajo, el ALTER correspondiente debe omitirse.

-- =================================================
-- 1) roper.is_active: 1 = activo, 0 = inactivo (soft-delete para ropers)
-- Añadir sólo si no existe (revisar pragma_table_info antes de ejecutar si la BD es existente).
-- NOTE: Do NOT wrap this file in an explicit BEGIN/COMMIT because sqlx::migrate!
-- runs migrations inside its own transaction. Having BEGIN/COMMIT here causes
-- "cannot start a transaction within a transaction" errors.
-- =================================================
ALTER TABLE roper ADD COLUMN is_active INTEGER NOT NULL DEFAULT 1;
UPDATE roper SET is_active = 1 WHERE is_active IS NULL;

-- =================================================
-- 2) payoff_rule.is_active: 1 = activo, 0 = inactivo (soft-delete para reglas de payoff)
-- =================================================
ALTER TABLE payoff_rule ADD COLUMN is_active INTEGER NOT NULL DEFAULT 1;
UPDATE payoff_rule SET is_active = 1 WHERE is_active IS NULL;

-- =================================================
-- 3) Normalizaciones menores
-- Asegurarse de que team.status no sea NULL (normalizar a 'active')
-- =================================================
UPDATE team SET status = 'active' WHERE status IS NULL;

-- =================================================
-- INSTRUCCIONES / VERIFICACIÓN (manual):
-- Antes de ejecutar esta migración sobre una BD existente, puedes comprobar si las columnas ya
-- existen con:
--   SELECT name FROM pragma_table_info('roper') WHERE name = 'is_active';
--   SELECT name FROM pragma_table_info('payoff_rule') WHERE name = 'is_active';
-- Si devuelven filas, comenta u omite el ALTER correspondiente.

