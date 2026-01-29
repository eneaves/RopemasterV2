# Documentación del backend (Tauri) — roping-manager-tauri

Última actualización: 14 de noviembre de 2025

Esta documentación describe la parte "backend" que corre dentro de la app Tauri (carpeta `src-tauri/`). Incluye: lista completa de comandos Tauri expuestos (API), tipos/payloads, esquema de base de datos, cómo ejecutar y recomendaciones.

---

## Resumen rápido

- Backend integrado en la app Tauri (no hay servidor HTTP separado).
- Código principal en `src-tauri/src/lib.rs` y `src-tauri/src/main.rs`.
- Persistencia: SQLite local gestionado con `sqlx` y migraciones en `src-tauri/migrations/`.
- Pool de conexiones: `SqlitePool` con WAL y `foreign_keys = true`.
- Los comandos que el frontend invoca están marcados con `#[tauri::command]` y registrados en `tauri::generate_handler!`.

---

## Cómo ejecutar (desarrollo)

1) Instala dependencias JS (si aún no lo hiciste):

```bash
npm install
```

2) Ejecuta el frontend (Vite):

```bash
npm run dev
```

3) Ejecuta la app Tauri (backend Rust integrado) en modo desarrollo:

```bash
npm run tauri dev
# o alternativamente
cargo tauri dev --manifest-path src-tauri/Cargo.toml
```

Notas:
- La base de datos SQLite se crea en el directorio dev local de la app (usando `app.path().app_local_data_dir()`); `lib.rs` imprime la ruta de la DB al arrancar.
- Las migraciones en `src-tauri/migrations/` se ejecutan automáticamente en arranque.

---

## Dependencias relevantes (extraídas de `src-tauri/Cargo.toml`)

- tauri (v2)
- sqlx 0.7 (features: runtime-tokio, sqlite, macros, migrate, uuid, chrono)
- tokio (rt-multi-thread)
- serde / serde_json
- chrono, uuid
- argon2 (hashing, presente pero no usado en `lib.rs` actualmente)
- tracing, tracing-subscriber
- rand
- ed25519-dalek, base64 (posible uso para firma/licencias)
- rust_xlsxwriter, csv (exportes)
- dirs-next

---

## Esquema de base de datos (migraciones resumidas)

Las migraciones están en `src-tauri/migrations/`.

Tablas principales (resumen):

- `app_user` (id, email UNIQUE, full_name, password_hash, is_active, created_at, updated_at)
- `role` (id, name UNIQUE)
- `user_role` (user_id, role_id)
- `audit_log` (user_id, action, entity_type, entity_id, metadata, created_at)

- `license_info` (device_id, payload_json, signature_b64, valid_from, valid_until, is_valid)
- `activity` (user_id, kind, details, created_at)

- `series` (id, name, season, status ∈ {active, upcoming, archived}, start_date, end_date, is_deleted)
- `event` (id, series_id FK, name, date, status ∈ {active, upcoming, completed, locked}, rounds [1..10], location, entry_fee, prize_pool, max_team_rating, is_deleted)
- `roper` (id, first_name, last_name, specialty ∈ {header, heeler, both}, rating REAL, phone, email, level ∈ {pro, amateur, principiante})
- `team` (id, event_id FK, header_id FK → roper, heeler_id FK → roper, rating, status ∈ {active, inactive}, UNIQUE(event_id, header_id, heeler_id))

- `draw` (event_id, round, position, team_id, UNIQUE(event_id, round, position))
- `run` (event_id, team_id, round, position, time_sec, penalty, total_sec, no_time, dq, status ∈ {pending, completed, skipped}, captured_by → app_user)
- `payoff_rule`, `payoff`

Constraints, triggers y notas:
- CHECKs en status, rounds, flags.
- Migración `0003` añade `roper.level` con triggers para validar valores permitidos.
- Seeds: roles `admin`, `operator`, `viewer`.

---

## API Tauri expuesta (comandos y tipos)

Todos los comandos son funciones anotadas con `#[tauri::command]` y reciben `State<'_, Db>` para acceder al pool SQLite.

Pauta: enumero el comando, firma (parámetros importantes) y comportamiento / validaciones principales.

---

### Health

- `health_check(db: State<'_, Db>) -> Result<String, String>`
  - Verifica SELECT 1 y retorna "ok" si la DB responde.

---

### Series

- `list_series(db) -> Result<Vec<SeriesRow>, String>`
  - Retorna series no borradas (is_deleted = 0).

- `create_series(db, payload: NewSeries) -> Result<i64, String>`
  - Inserta nueva serie.
  - NewSeries: { name: String, season: String, status: String, start_date: Option<String>, end_date: Option<String> }

- `update_series(db, id: i64, patch: UpdateSeries) -> Result<(), String>`
  - Verifica existencia; actualiza campos opcionales.
  - UpdateSeries: { name?: String, season?: String, status?: String, start_date?: Option<String> | null, end_date?: Option<String> | null }
  - Valida status: 'active' | 'upcoming' | 'archived'

- `delete_series(db, id: i64) -> Result<(), String>`
  - No permite eliminación si hay eventos `locked`.
  - Soft-delete de series y eventos asociados en transacción.

---

### Events

- `list_events(db, series_id: Option<i64>) -> Result<Vec<EventRow>, String>`
  - EventRow incluye: id, series_id, name, date, status, rounds, location, entry_fee, prize_pool, max_team_rating, created_at, updated_at

- `list_all_events_raw(db) -> Result<Vec<EventRow>, String>`
  - Retorna todos los eventos sin filtro `is_deleted`.

- `create_event(db, payload: NewEvent) -> Result<i64, String>`
  - NewEvent: { series_id: i64, name: String, date: String, rounds: i64, status: Option<String>, location: Option<String>, entry_fee: Option<f64>, prize_pool: Option<f64> }
  - Normaliza status desde FE a valores permitidos ('draft' -> 'upcoming', 'finalized' -> 'completed').

- `update_event_status(db, id: i64, status: String) -> Result<(), String>`
  - Actualiza status y updated_at.

- `update_event(db, id: i64, patch: EventPatch) -> Result<(), String>`
  - EventPatch: { name?: String, date?: String, rounds?: i64, status?: String, entry_fee?: f64, prize_pool?: f64, location?: String, max_team_rating?: i64 }
  - Verifica existencia y `ensure_event_unlocked` (no permitir cambios si locked).
  - Usa QueryBuilder para updates dinámicos.

- `delete_event(db, id: i64) -> Result<(), String>`
  - Intenta soft-delete (is_deleted); fallback: set status = 'archived'.

- `duplicate_event(db, id: i64) -> Result<i64, String>`
  - Prohíbe duplicar si status == 'locked'.
  - Inserta copia con `name (Copy)` y status 'upcoming'.

- `lock_event(db, event_id: i64) -> Result<(), String>`
  - Cambia status = 'locked'.

---

### Runs / Capture

- `save_run(db, payload: SaveRun) -> Result<i64, String>`
  - SaveRun: { event_id: i64, team_id: i64, round: i64, position: i64, time_sec: Option<f64>, penalty: f64, no_time: bool, dq: bool, captured_by: Option<i64> }
  - Calcula `total_sec` = time_sec + penalty a menos que `no_time` o `dq`.
  - Inserta o actualiza (ON CONFLICT(event_id, round, team_id) DO UPDATE).

- `get_runs(db, event_id: i64, round: Option<i64>) -> Result<Vec<RunRow>, String>`
  - Devuelve runs filtradas por event y opcionalmente por round.

---

### Teams

- `list_teams(db, event_id: i64) -> Result<Vec<TeamRow>, String>`
  - Retorna equipos `active` del evento.

- `create_team(db, NewTeam) -> Result<i64, String>`
  - NewTeam: { event_id: i64, header_id: i64, heeler_id: i64, rating: f64 }
  - Valida evento no locked, header != heeler, existencia de ropers, y respeta UNIQUE(event_id, header_id, heeler_id).

- `update_team(db, UpdateTeam) -> Result<(), String>`
  - UpdateTeam: { id: i64, rating?: f64, status?: String }
  - Valida evento del team no locked.

- `delete_team(db, id: i64) -> Result<(), String>`
  - Borrado duro; valida lock en evento.

- `hard_delete_teams_for_event(db, event_id: i64) -> Result<(), String>`
  - Borra todos los equipos de un evento (requiere que el evento no esté locked).

---

### Ropers

- `list_ropers(db) -> Result<Vec<RoperRow>, String>`

- `create_roper(db, NewRoper) -> Result<i64, String>`
  - NewRoper: { first_name: String, last_name: String, specialty: String, rating: i64, phone?: String, email?: String, level?: String }
  - Valida specialty ∈ {header, heeler, both} y level ∈ {pro, amateur, principiante}.

- `update_roper(db, UpdateRoper) -> Result<(), String>`
  - UpdateRoper: { id: i64, first_name?: String, last_name?: String, specialty?: String, rating?: i64, phone?: String, email?: String, level?: String }
  - Valida valores permitidos; usa QueryBuilder.

- `delete_roper(db, id: i64) -> Result<(), String>`
  - No permite eliminar si está referido por `team`.

---

### Draw / Standings

- `generate_draw(db, opts: GenerateDrawOptions) -> Result<i64, String>`
  - GenerateDrawOptions: { event_id: i64, round: i64, reseed?: bool, seed_runs?: bool }
  - Valida evento no locked; obtiene equipos activos; baraja si `reseed`; upserta filas en `draw` y, si `seed_runs`, crea/actualiza `run` pendientes.

- `get_draw(db, event_id: i64, round: i64) -> Result<Vec<DrawRow>, String>`
  - Devuelve draw con información de header/heeler (JOIN team).

- `get_standings(db, event_id: i64) -> Result<Vec<StandingRow>, String>`
  - Agrega runs por equipo y genera ranking (reglas: completed_runs desc, total_time asc, best_time asc, team_id asc).

---

## Observaciones de seguridad y control de acceso

- No hay endpoints de autenticación/autoridad expuestos en `lib.rs` (aunque `app_user`, `role`, `user_role` existen en el esquema y `argon2` está en `Cargo.toml`).
- Actualmente los comandos Tauri están disponibles para la UI local; si tu app necesita multiusuario o separación por roles deberías implementar login + autorización y filtrar comandos según rol.
- `audit_log` existe pero no es alimentado por las funciones actuales; se recomienda insertar registros en operaciones críticas.

---

## Recomendaciones y próximos pasos (priorizadas)

1. Implementar autenticación y control de accesos:
   - Comandos: `create_user`, `login`, `logout`, `get_current_user`, `assign_role`.
   - Usar `argon2` para hashear contraseñas y almacenar `password_hash` en `app_user`.
   - Restringir comandos sensibles según `user_role`.

2. Añadir escritura en `audit_log` en operaciones mutativas (create/update/delete) para trazabilidad.

3. Añadir tests de integración en Rust:
   - Levantar SQLite en memoria y probar operaciones críticas: `create_team`, `generate_draw`, `save_run`, `get_standings`.

4. Documentar los tipos en TypeScript para el frontend (interfaz de IPC), o generar documentación automática (OpenAPI no aplica directamente a Tauri IPC, pero puedes generar un JSON con la lista de handlers y sus firmas).

5. Si se necesita licenciamiento o firmas, añadir endpoints para validar `license_info` con las librerías (`ed25519-dalek`, `base64`).

6. Considerar instrumentación de métricas o logs persistentes (tracing + tracing-appender) para debugging en producción.

---

## Archivo de referencia (ubicaciones importantes)

- Código principal: `src-tauri/src/lib.rs`
- Entrypoint bin: `src-tauri/src/main.rs`
- Migraciones: `src-tauri/migrations/`
- Cargo manifest: `src-tauri/Cargo.toml`
- Frontend: `src/` (comunicaciones via `@tauri-apps/api` desde React/TS)

---

## Anexos: ejemplos de payloads (TypeScript-like)

Ejemplos prácticos para el frontend al invocar comandos Tauri (usando `@tauri-apps/api` invoke):

- Crear serie:

```ts
const payload = {
  name: "Serie Verano",
  season: "2025",
  status: "upcoming",
  start_date: "2025-06-01",
  end_date: "2025-09-01"
};
const id = await invoke<number>('create_series', { payload });
```

- Crear equipo:

```ts
const team = { event_id: 1, header_id: 10, heeler_id: 11, rating: 1200.0 };
const teamId = await invoke<number>('create_team', { t: team });
```

- Guardar run:

```ts
const run = { event_id: 1, team_id: 5, round: 1, position: 2, time_sec: 12.34, penalty: 0.5, no_time: false, dq: false, captured_by: 1 };
const runId = await invoke<number>('save_run', { payload: run });
```

---

## Verificación que realicé en esta sesión

- Lectura y análisis de `src-tauri/src/lib.rs` y `main.rs`.
- Lectura de migraciones `0001_init.sql`, `0002_add_max_team_rating.sql`, `0003_add_roper_level.sql`.
- Lectura de `src-tauri/Cargo.toml` y `package.json`.
- Extraje y documenté la lista completa de comandos Tauri y tipos asociados.

