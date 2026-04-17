# Plan de Licenciamiento Offline — Fase 0

## 1. Resumen ejecutivo
La app cliente ya cuenta con cimientos básicos de licenciamiento dentro de `src-tauri/src/license`, pero la lógica está fuertemente acoplada al runtime de Tauri (paths, dialogs, estado global) y no cubre los nuevos requisitos: `installation_id`, `installation_pubkey`, snapshots reutilizables y un runtime desacoplado de la UI. Esta fase define cómo introducir un servicio interno de licencias que reutilice `crates/license_core` y el análisis previo del licgenerator sin reescribirlo, manteniendo la app compilable e incremental.

La estrategia es aislar un **LicenseRuntime** autocontenido que viva en el backend (`src-tauri`) y exponga operaciones claras (binding, storage, validación, request/export/import). Sobre esa capa se conectarán gradualmente las pantallas existentes (`LicenseGate`, `LicensePanel`) y los guards de comandos. Fase 1 se concentrará en crear ese runtime base (persistencia de binding, instalación y snapshots) sin UI adicional.

## 2. Estado actual de la app respecto a licenciamiento
- **Backend Tauri**: `src-tauri/src/license` contiene `device.rs`, `commands.rs`, `storage.rs`, `validator.rs` y `mod.rs`. Genera device hash, crea `.req` básicos, instala licencias (`install_license`), almacena raw bytes en SQLite (`license` table) y mantiene `LicenseState` en memoria. `public_key_dev.der` embebe la llave pública de verificación.
- **Bootstrap**: En `src-tauri/src/lib.rs` (setup, línea ~3570) se ejecuta `license::bootstrap(...)` que carga la licencia almacenada y llena el cache global antes de registrar el `Db` que hace `require_license()` en prácticamente todos los comandos.
- **Frontend**: `src/providers/LicenseProvider.tsx` obtiene el estado vía `license_status` (`src/lib/api.ts`). `src/App.tsx` y `src/components/LicenseGate.tsx` bloquean toda la UI cuando la licencia no está activa. `src/components/LicensePanel.tsx` expone generación `.req` e importación `.lic` usando diálogos Tauri.
- **Almacenamiento local**: Se usa `app_config_dir` para `device/device_id.bin`, `licenses/requests`, `licenses/installed/current.lic` + historial, y SQLite para el blob crudo. No existen snapshots versionados ni carpetas explícitas para runtime state.
- **Contrato actual**: `crates/license_core::request::LicenseRequest` solo envía `device_hash`, `plan`, `nonce`, `customer_name_hint`. No hay `installation_id` ni `installation_pubkey`.

## 3. Piezas reutilizables
- `crates/license_core`: parsing/serialización de `.req`, verificación Ed25519 y `LicensePayload` + policies.
- `src-tauri/src/license/validator.rs`: clasificación runtime (`Active`, `Expired`, etc.) y mapeo de errores reutilizable dentro del nuevo runtime.
- `LicenseState` + `ensure_active` (`src-tauri/src/license/mod.rs`) como cache read-mostly que puede envolverse dentro del nuevo servicio.
- `storage.rs` y la tabla `license` que ya persisten el blob exacto; se extenderán con metadatos pero no se descartan.
- UI existente (`LicenseGate`, `LicensePanel`, provider) lista para Fase 5: no se modifica ahora pero ya consume DTOs alineados con `license_core`.
- Documentación previa (`docs/client-license-integration-analysis.md`) que describe flujos del licgenerator y será referencia para validar compatibilidad.

## 4. Piezas faltantes
- `DeviceBindingStore` formal que genere y persista **installation_id**, **installation_keypair** (solo publica `installation_pubkey`) y el fingerprint/dispositivo actual; hoy solo existe `device_id.bin`.
- Definición clara de `installation_pubkey` en el request y almacenamiento seguro de la clave privada asociada al dispositivo (uso local).
- Directorio de snapshots (`licenses/snapshots/`) que guarde el último estado verificado, hashes y timestamps para detección de corrupción.
- `LicenseRuntime` reusable (sin dependencias directas de Tauri dialogs/UI) con API síncrona/async para `init`, `generate_request`, `install_license`, `status`, `remove`, `export_req`, `import_lic`.
- Tipos internos (`LicenseStatusInternal`, `LicenseSnapshot`, `LicenseRuntimeError`) que desacoplen el dominio del DTO UI.
- Registro de llaves públicas de verificación (keyring) para permitir rotación controlada en el cliente en vez de un único `.der` hardcodeado.
- Hooks explícitos para arranque (lógica en `bootstrap`) y para bloqueo funcional sin replicar `db.require_license()` en cada comando.

## 5. Propuesta de módulos/servicios internos
| Módulo | Ubicación sugerida | Responsabilidad |
| --- | --- | --- |
| `license/runtime/mod.rs` | `src-tauri/src/license/runtime/mod.rs` | Punto de entrada; expone `LicenseRuntime` y administra el ciclo de vida (init, guard, status). |
| `license/runtime/device_binding.rs` | Nuevo archivo | Implementa `DeviceBindingStore`: genera `installation_id` (UUID v4) y un par Ed25519, guarda `installation_pubkey` y fingerprint (`device_hash`) en `app_config_dir/device/installation.json`. |
| `license/runtime/storage.rs` | Extensión del actual | Abstrae persistencia de licencias (`active/current.lic`), snapshots (`snapshots/*.json`) y metadatos (`installed_at`, `last_verified_at`). Mantiene compatibilidad con SQLite. |
| `license/runtime/state.rs` | Nuevo | Define `InstallationState`, `LicenseSnapshot`, `LicenseRuntimeStatusInternal`, `LicenseCache`. |
| `license/runtime/service.rs` | Nuevo | Coordina binding + storage + validator. Solo depende de trait `LicenseKeyring` (para llaves públicas) y de `license_core`. |
| `license/runtime/keyring.rs` | Nuevo | Gestiona llaves públicas de verificación (prod/dev) y expone la correcta según `app_id`/entorno; evita hardcodear bytes en múltiples archivos. |
| `license/runtime/events.rs` (opcional) | Nuevo | Envía notificaciones internas (e.g., `LicenseStateChanged`) para que la UI refresque sin pooling futuro. |

**Storage local**
- `app_config_dir/device/installation.json`: `{ installation_id, installation_pubkey, device_hash, created_at }`.
- `app_config_dir/licenses/requests/*.req`: se conserva.
- `app_config_dir/licenses/installed/current.lic` + `history/`: se conserva.
- `app_config_dir/licenses/snapshots/*.json`: nuevo historial con hash SHA-256 del `.lic`, `last_verified_at` y resumen de policies para validaciones offline.

**Integración**
- `license::bootstrap` cargará via `LicenseRuntime::load_from_storage()` y poblará `LicenseState`.
- `Db::require_license()` seguirá delegando en `LicenseRuntime::ensure_active()`.
- Los comandos Tauri (`get_device_hash`, `generate_license_request`, etc.) se reescribirán gradualmente para usar el runtime sin exponer detalles de storage.

## 6. Flujo de generación de `.req`
1. UI o CLI invoca `generate_license_request(plan, hint, destination)`.
2. `LicenseRuntime::generate_request` asegura que `DeviceBindingStore` exista. Si falta, genera `installation_id` (UUID), `installation_keypair` (Ed25519) y `device_hash` (32 bytes aleatorios). Sólo `installation_pubkey` se expone fuera del store.
3. Se construye `LicenseRequest` reutilizando `license_core::request::request_to_bytes`, extendido en futuras fases para incluir `installation_id`, `installation_pubkey` y fingerprint. El runtime valida internamente que `app_id` y `plan` sean aceptados.
4. El runtime serializa a `.req`, lo guarda en `licenses/requests/<timestamp>-<plan>-<hash>.req` y, si se solicitó, lo exporta a la ruta del usuario usando `write_atomic`.
5. Devuelve un `LicenseRequestSummary` con hash del dispositivo, `installation_id`, `installation_pubkey`, nonce y paths guardados para que la UI los muestre.

## 7. Flujo de importación de `.lic`
1. UI proporciona `LicenseInputPayload` (`path` o `bytes`).
2. `LicenseRuntime::import_license` lee los bytes, valida tamaño/estructura (`license_core::verify_license`) y obtiene el payload.
3. `DeviceBindingStore` entrega el `device_hash` + `installation_id`; `validator::runtime_state` confirma binding (`app_id`, fingerprint, policies, window). Si el estado no es `Active`, se devuelve error específico.
4. Se calcula SHA-256 para snapshot y se guarda junto con `installed_at`, `last_verified_at` y resumen de policies en `snapshots/<ts>.json`.
5. Los bytes se guardan atómicamente en `licenses/installed/current.lic` y en SQLite (`license` table). Se crea entrada en `history/`.
6. `LicenseState` en memoria se actualiza y se emite evento interno para que `LicenseProvider` refresque.

## 8. Flujo de validación al arranque
1. Durante `tauri::Builder::setup`, `LicenseRuntime::init(handle, pool)` carga `installation.json` y crea uno nuevo si no existe.
2. Se lee `licenses/installed/current.lic` (o el blob en SQLite) y se verifica con la llave pública configurada (keyring) y el fingerprint actual.
3. Se consulta el snapshot más reciente para comparar `hash` y `last_verified_at`. Si hay divergencia, se marca `needs_revalidation` y se fuerza una evaluación completa.
4. Los resultados alimentan `LicenseState` (`Arc<RwLock<Option<LicenseCache>>>`) para que `Db::require_license()` pueda bloquear operaciones.
5. Cualquier error borra el cache y deja registro para UI; no se intentará reemitir licencias.

## 9. Puntos donde se bloqueará funcionalidad
- **Backend**: `Db::require_license()` (`src-tauri/src/lib.rs`) seguirá antes de cada comando sensible (series, eventos, equipos, exportaciones, etc.). La implementación se moverá a `LicenseRuntime::ensure_active()` para unificar validaciones.
- **UI global**: `src/App.tsx` renderiza `LicenseGate` cuando `LicenseProvider` reporta estado distinto de `active`.
- **Panel de settings**: `src/components/SettingsManagement.tsx` muestra estado y permite acciones manuales, y seguirá usando los hooks.
- **Runtime guards internos**: En Fase 4 se añadirá un guard middleware (por ejemplo, `LicenseGuard<T>` que envuelve handlers) para evitar olvidar el check en nuevos comandos.

## 10. Riesgos técnicos
- **Desalineación de contrato `.req`/`.lic`**: agregar `installation_id`/`installation_pubkey` requiere actualizar `crates/license_core` y el licgenerator simultáneamente. Mitigación: introducir campos opcionales/compatibles y versionar `REQUEST_VER`.
- **Corrupción de binding**: Si el usuario borra `installation.json`, se generará un nuevo binding y las licencias anteriores quedarán inválidas. Se documentará y se guardarán copias en `snapshots` con hash para detectar cambios.
- **Clock skew**: Validaciones offline dependen del reloj del sistema; se deben loggear desvíos y mostrar mensajes claros (ya contemplado en `LicenseRuntimeStatus`).
- **Rotación de llaves públicas**: Actualmente solo existe `public_key_dev.der`. Necesitamos un keyring para soportar múltiples llaves y ambientes sin recompilar toda la app.
- **Sincronización UI-backend**: `LicenseProvider` depende de `license_status`; cualquier cambio en DTOs debe ser backward compatible o versionado.
- **Acceso concurrente a storage**: exportar `.req`/importar `.lic` mientras otro hilo escribe snapshots puede generar condiciones de carrera; se utilizarán locks (Mutex) alrededor de `LicenseRuntime`.

## 11. Plan detallado de Fase 1
1. **Crear módulo `license/runtime`** con estructura básica (`mod.rs`, `state.rs`, `errors.rs`). Integrarlo en `src-tauri/src/license/mod.rs` sin modificar UI.
2. **Implementar `DeviceBindingStore`** (`device_binding.rs`): genera/persiste `installation_id`, `installation_keypair` (ed25519) y `device_hash`; expone métodos `installation_id()`, `installation_pubkey()`, `device_hash()`.
3. **Extender `storage.rs`** para soportar snapshots y un `LicenseStorage` trait (SQLite + filesystem). Mantener la tabla existente y añadir escritura de snapshots JSON.
4. **Construir `LicenseRuntime`** (`service.rs`) que combine binding + storage + validator y exponga `init()`, `current_status()`, `ensure_active()`. Integrarlo en `license::bootstrap` y en `Db::require_license()`.
5. **Actualizar comandos internos** (`commands.rs`) para depender del runtime a través de `State<'_, LicenseRuntime>` pero sin cambiar sus firmas públicas todavía.
6. **Pruebas y documentación**: añadir pruebas unitarias para `DeviceBindingStore` y `LicenseRuntime` (mock storage), y documentar directorios/formatos en `docs/README`.

## 12. Criterios de aceptación para Fase 1
- La app compila en Desktop y mantiene el flujo actual (no hay UI nueva).
- `installation.json` se crea una sola vez y conserva `installation_id`, `installation_pubkey` y `device_hash` entre ejecuciones.
- `LicenseRuntime` expone `ensure_active()` y `current_status()` reutilizados por `Db::require_license()` y `license_status` sin regresiones visibles.
- Existen pruebas unitarias para `DeviceBindingStore` (genera/recupera binding) y para el runtime (inicialización sin licencia debería retornar `None`).
- El almacenamiento de licencias sigue guardando el blob crudo y ahora crea snapshots JSON con hash + timestamps.
- Documentación actualizada (`docs/app-license-phase-0-plan.md`) describe arquitectura y límites de la fase.
