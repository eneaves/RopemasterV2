# Reiniciar la base de datos en desarrollo

Fecha: 14 de noviembre de 2025

Este documento explica, paso a paso, cómo resetear la base de datos SQLite de desarrollo para que las migraciones se apliquen desde cero (0001 → 0004). Está diseñado para entornos de desarrollo donde *no* hay datos importantes a preservar.

Resumen rápido (checklist):

1. Localizar la ruta del archivo de la base de datos (se imprime al arrancar la app o usar `find`).
2. Parar la app y borrar (o mover) el archivo `.db`/`.sqlite` encontrado.
3. Ejecutar la app en modo desarrollo (`npm run tauri dev`) para que `sqlx::migrate!` aplique todas las migraciones en una DB nueva.
4. Verificar manualmente que las tablas y columnas esperadas existen y probar operaciones CRUD para confirmar la política de soft-delete.

---

Detalle de pasos

## 1) Localizar ruta de la DB

La app Tauri (archivo `src-tauri/src/lib.rs`) resuelve la ruta de la DB usando `app.path().app_local_data_dir()` y escribe al arrancar, en stderr, una línea del tipo:

```
DB path -> /Users/<tu-usuario>/Library/Application Support/<AppName>/roping_manager.db
```

Otras formas de localizarla:

- Arranca la app en modo desarrollo (ver paso 3). Revisa la consola donde lanzaste `npm run tauri dev` y busca la línea "DB path ->".
- Si no ves el log, busca el archivo por nombre con `find` (puede tardar si tienes muchos ficheros):

```bash
# buscar en tu home (macOS / Linux)
find ~ -type f -name 'roping_manager.db' 2>/dev/null
```

- En macOS la ruta típica es:

```
~/Library/Application Support/<AppName>/roping_manager.db
```

Nota: sustituye `<AppName>` por el nombre de la app si lo conoces; si no, simplemente usa `find`.

## 2) Parar la app y borrar el archivo de DB (o mover como backup)

Asegúrate de que la app no está corriendo. Luego borra (o mueve) el archivo. Ejemplo (zsh / macOS):

```bash
# si quieres borrar (irrecuperable)
rm /ruta/completa/a/roping_manager.db

# o mover a respaldo por si acaso
mv /ruta/completa/a/roping_manager.db ~/Desktop/roping_manager.db.bak
```

Recomendación: si hay alguna duda, haz un `mv` a una carpeta de respaldo en lugar de `rm`.

## 3) Volver a ejecutar la app para aplicar migraciones (0001 → 0004)

Desde la raíz del proyecto, lanza la app en modo desarrollo:

```bash
# instalar dependencias (si no lo has hecho)
npm install

# arrancar tauri (esto compilará Rust y ejecutará sqlx::migrate! en arranque)
npm run tauri dev
# o alternativamente
cargo tauri dev --manifest-path src-tauri/Cargo.toml
```

Comportamiento esperado:
- La app creará la carpeta de datos locales (si no existe) y el archivo `roping_manager.db` nuevo.
- `sqlx::migrate!` ejecutará en orden las migraciones encontradas en `src-tauri/migrations/` (0001_init.sql, 0002_add_max_team_rating.sql, 0003_add_roper_level.sql, 0004_soft_delete_policy.sql).
- En la consola deberías ver logs y la línea con "DB path -> ...".

Si algo falla (por ejemplo, la migración 0004 contiene un `ALTER TABLE` para una columna ya existente), verás el error en la salida; en ese caso puedes:
- Restaurar el backup y ajustar la migración (si es necesario), o
- Borrar la BD y reintentar (este documento asume que no tienes datos importantes).

### Alternativa: aplicar migraciones manualmente con `sqlx-cli`

Si prefieres aplicar migraciones manualmente sin arrancar la app:

```bash
# instalar sqlx-cli (si no lo tienes ya)
cargo install sqlx-cli --no-default-features --features sqlite

# crear una db vacía
sqlite3 /ruta/a/roping_manager.db "VACUUM;"

# aplicar migraciones (ubicadas en src-tauri/migrations)
SQLX_OFFLINE=true sqlx migrate run --source src-tauri/migrations -p src-tauri/Cargo.toml
```

Nota: `sqlx migrate run` necesita conocer la URL de conexión si no está en `DATABASE_URL`; para SQLite la opción simple es usar el archivo y `--source` según la versión de `sqlx-cli`. En la práctica arrancar la app es más sencillo.

## 4) Pruebas manuales rápidas (checklist)

Realiza estas acciones desde la UI (o invocando comandos Tauri) para comprobar la política de borrado:

- Series:
  - Crear una serie nueva.
  - Borrar la serie (usando la acción de la UI que llama a `delete_series`).
  - Verificar que la serie NO desaparece de la tabla a nivel SQL pero que `series.is_deleted = 1`.
  - Verificar que `list_series` (invocado desde UI) ya no muestra la serie.

- Events:
  - Crear un evento dentro de una serie.
  - Borrar el evento.
  - Verificar que `event.is_deleted = 1` y `event.status = 'archived'`.
  - Verificar que `list_events` no muestra eventos borrados; `list_all_events_raw` sí.

- Teams / Ropers:
  - Crear ropers y equipos.
  - Borrar un roper (UI llama a `delete_roper`) → `roper.is_active` debe ser 0.
  - Intentar crear un equipo con un roper inactivo (si no se validó en backend, esto puede permitirlo; recomendamos añadir validación). Recomendación: validar en `create_team` que ambos ropers tengan `is_active = 1`.
  - Borrar un team → `team.status` debe pasar a `'inactive'` y `list_teams` no debe mostrarlo.

### Comprobaciones SQL rápidas (útiles desde terminal)

Puedes inspeccionar la DB con `sqlite3` o cualquier cliente SQLite. Ejemplos:

```bash
# abrir sqlite3 shell
sqlite3 /ruta/a/roping_manager.db

# comprobar series soft-deleted
SELECT id, name, is_deleted FROM series ORDER BY id DESC LIMIT 20;

# comprobar eventos borrados
SELECT id, name, status, is_deleted FROM event ORDER BY id DESC LIMIT 20;

# comprobar ropers activos
SELECT id, first_name, last_name, is_active FROM roper ORDER BY last_name, first_name LIMIT 50;

# comprobar teams activos
SELECT id, event_id, header_id, heeler_id, status FROM team WHERE event_id = <tu_event_id> ORDER BY id;

# comprobar payoff rules activas
SELECT id, event_id, position, percentage, is_active FROM payoff_rule WHERE is_active = 1;
```

## 5) Limpieza / extras

- Si necesitas purgar datos (hard delete) para desarrollo, hay funciones administrativas en `lib.rs` como `hard_delete_teams_for_event`. Úsalas con cuidado.
- Si quieres automatizar el reinicio en CI / desarrollo, podemos añadir un script en `package.json` que encuentre y borre la DB dev antes de arrancar.

Ejemplo de script en `package.json` (opcional):

```json
"scripts": {
  "tauri:reset": "node ./scripts/reset-db.js && npm run tauri dev"
}
```

Y `scripts/reset-db.js` podría buscar y borrar la DB de la ruta esperada.

---

Notas de seguridad y precaución

- Este procedimiento elimina datos locales irreversiblemente si borras el archivo con `rm` sin backup.
- No aplicar este procedimiento en producción.
- Si la migración 0004 falla en una BD existente (por columnas ya presentes), ajusta la migración o borra la BD según tu flujo.

---

¿Quieres que añada un script de utilidad para encontrar y borrar automáticamente la DB de desarrollo (por ejemplo `scripts/reset-db.js`) y opcionalmente un `npm` script que ejecute el reset + arranque? Puedo generarlo ahora y probarlo localmente (sin ejecutar la app).