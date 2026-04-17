# Client License Integration Analysis

## Flujo actual (generador / app Tauri)
- El hash de dispositivo se crea una sola vez como 32 bytes aleatorios y se guarda en `app_config_dir/device/device_id.bin`. Cada vez que se requiere se lee o regenera (`src-tauri/src/license/device.rs`).
- La solicitud `.req` se arma en memoria con `license_core::request::LicenseRequest` y se serializa en CBOR (`request_to_bytes`). El comando Tauri `generate_license_request` rellena `ver`, `app_id`, `plan`, `device_hash`, `nonce` y la pista opcional de cliente, lo guarda bajo `licenses/requests/<timestamp>-<plan>-<hash>.req` y opcionalmente en una ruta elegida por el usuario (`src-tauri/src/license/commands.rs`).
- La instalación de una licencia lee bytes desde disco, valida firma y payload con `license_core::verify_license` + `license::validator::runtime_state`, y solo acepta estado `Active`. Guarda los bytes crudos en SQLite (`license` table) y también en `licenses/installed/current.lic` + historial, manteniendo una caché en memoria (`LicenseState`).
- En cada arranque `bootstrap` carga el blob guardado, vuelve a evaluarlo contra el dispositivo/hora y actualiza la caché o borra registros si falla (`src-tauri/src/license/mod.rs`).
- Las llamadas del backend (`Db::require_license`) rechazan cualquier operación si `LicenseState` no tiene una licencia vigente.

## Flujo ideal para una app cliente
- Abstraer un `LicenseService` embebido que genere `.req`, exporte/importar archivos y mantenga estado en un directorio conocido (`~/.app/licenses`).
- Separar responsabilidades: `DeviceBinding` (genera/guarda hash), `RequestBuilder` (usa `license_core::request`), `LicenseStorage` (persistencia y snapshot), `LicenseValidator` (usa `license_core::verify_license` + runtime policy) y `LicenseStatusReporter` para la UI.
- Definir puntos de integración UI: pantalla de activación (copiar hash + exportar `.req`), importador `.lic`, estado global y bloqueos de funcionalidad basados en `LicenseRuntimeStatus`.

## Brechas entre ambos
1. El código actual depende de Tauri (`AppHandle`, rutas, dialogs) y SQLite; una app cliente embebida necesitará wrappers multiplataforma para paths, diálogos y almacenamiento.
2. No existe un módulo independiente tipo `licgen_workflows`: la lógica está repartida entre `license_core` (reutilizable) y comandos Tauri (acoplados). Se requiere aislar workflows en una librería reutilizable.
3. El generador actual asume que la validación completa sucede solo al instalar/arrancar; una app cliente debería hacer revalidaciones periódicas y exponer estados detallados al usuario.
4. Los errores (`CommandError`) están pensados para mensajes en español del CLI/UI actual; habría que mapear códigos/causas a mensajes/desiciones UX en la app objetivo.

## Tareas futuras
### Se puede reutilizar tal cual
- `crates/license_core`: parsing del `.lic`, verificación de firma Ed25519, validación de payload y serialización de `.req`.
- Estructuras de datos (`LicensePayload`, `LicenseRequest`, `LicenseRuntimeStatus`) para mostrar detalles y decisiones de negocio.
- Reglas de validación (planes permitidos, `app_id`, ventanas de vigencia, `max_clock_skew`).

### Requiere wrapper / adaptación
- Generación y almacenamiento del hash de dispositivo: replicar la estrategia (archivo binario) pero abstraída detrás de un trait para admitir diferentes plataformas.
- Persistencia del blob de licencia y snapshots: hoy usa SQLite + archivos; la app cliente podría usar archivos planos o el storage propio, pero debe conservar el patrón “guardar bytes exactos + metadata mínima”.
- Exportación/importación de `.req`/`.lic`: depende de diálogos Tauri. La app cliente debe proveer su propia UI/IO.
- Gestión de estado en memoria (`LicenseState`): reutilizar la idea pero integrarla con el gestor de estado propio (Redux, context, servicio Kotlin, etc.).

### Hay que crear desde cero
- Un módulo `LicenseWorkflow` o servicio que exponga métodos síncronos/async independientes del CLI para: `generate_request`, `export_request`, `import_license`, `validate_now`, `enforce_active`.
- Manejo de snapshots/historial alineado con los requisitos de la app (por ejemplo, almacenar `last_verified_at`, `installed_at` y log de cambios en la DB principal o en archivos).
- UX de activación (pasos guiados, indicadores, mensajes de error configurables) integrada con el flujo funcional de la app final.
