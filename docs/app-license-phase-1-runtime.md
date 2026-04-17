# Licenciamiento Offline — Fase 1

## Objetivo
Implementar la capa base del runtime de licencias para la app cliente sin modificar la interfaz pública de los comandos Tauri ni los formatos `.req/.lic`. Esta fase entrega la infraestructura para binding de dispositivo, manejo de estado interno y un keyring flexible.

## Componentes introducidos
- **`license/runtime/`**: árbol modular con `device_binding`, `keyring`, `service`, `state` y `errors`.
- **`DeviceBindingStore`**: persiste `installation_id`, `device_hash` y placeholder de `installation_pubkey` en `app_config_dir/device/installation.json`, migrando automáticamente desde `device_id.bin` si existe.
- **`LicenseRuntime`**: orquesta keyring, binding y cache (`LicenseState`) sin romper a los consumidores actuales; expone métodos auxiliares usados por los comandos.
- **Keyring embebido**: se encapsula la llave pública en `runtime::keyring`, permitiendo rotaciones futuras sin tocar el resto del código.
- **Storage adaptado**: `storage.rs` ahora conoce los directorios (`licenses/requests`, `installed`, `snapshots`) y la persistencia de archivos, preparando el terreno para snapshots offline.

## Flujo actualizado
1. **Bootstrap**: `license::bootstrap` crea un `DeviceBindingStore`, instancia `LicenseRuntime` con el keyring por defecto y sincroniza el cache desde SQLite.
2. **Comandos Tauri**: `get_device_hash`, `generate_license_request`, `install_license`, `license_status` y `remove_license` reciben `State<LicenseRuntime>` y usan sus métodos para obtener hash, llave pública y cache. Las firmas públicas de los comandos siguen iguales desde el frontend.
3. **Storage**: los helpers de rutas/snapshots viven en `storage.rs` para que el runtime y futuros servicios los reutilicen.

## Decisiones clave
- **`LicenseState` como fachada**: se mantiene `LicenseState` para no tocar `Db::require_license()` ni otros consumidores. El runtime es el único responsable de mutar ese estado, dejando claro que `LicenseState` es un shim temporal.
- **Contrato `.req` intacto**: el runtime conoce `installation_id`/`installation_pubkey`, pero no los añade al request todavía. Esto evita desalinear `REQUEST_VER`. La próxima fase deberá coordinar con el licgenerator para incluir dichos campos.
- **Keyring mínimo**: sólo se introdujo una interfaz simple y un keyring embebido; no hay lógica compleja de rotación aún.

## Pruebas añadidas
- **`DeviceBindingStore`**: crea nueva instalación y migra desde `device_id.bin`, asegurando persistencia estable.
- **`LicenseRuntime`**: verifica que `ensure_active` falle sin cache y funcione cuando existe una licencia válida en memoria.

## Riesgos pendientes
- Aún no se serializa `installation_id`/`installation_pubkey` en `.req`; se documentó para Fase 2.
- El runtime todavía delega la generación/exportación de `.req` y la instalación de `.lic` a la lógica anterior, aunque ya usa binding y keyring nuevos.
- No se han implementado snapshots ni guards adicionales; sólo se preparó la estructura.

## Próximos pasos sugeridos
1. Actualizar el contrato de `LicenseRequest` para incluir `installation_id`/`installation_pubkey`, una vez coordinado con el licgenerator.
2. Aprovechar `snapshot_dir` para almacenar hash/estado en disco y endurecer la validación offline.
3. Exponer métodos del runtime para generación/exportación (`generate_request`) y manejo de snapshots antes de añadir UI en fases posteriores.
