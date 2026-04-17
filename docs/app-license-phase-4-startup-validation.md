# Licenciamiento Offline — Fase 4 (Validación en arranque y guard)

## Resumen ejecutivo
- `LicenseRuntime` se formaliza como la **única** fuente de verdad del estado de licencia. `LicenseState` permanece solo como caché/shim para consumidores heredados.
- El arranque de la app carga automáticamente la licencia persistida, la evalúa offline y actualiza `RuntimeInfo` + `LicenseState`.
- `Db::require_license()` ahora delega en `LicenseRuntime::ensure_active()`, que mapea determinísticamente cada estado de `RuntimeInfo` a errores de guard conocidos.
- Estados faltantes, inválidos, expirados o con device mismatch bloquean funcionalidades protegidas, pero no eliminan el blob persistido (solo se limpia el cache activo).

## Flujo de validación al arranque
1. `license::bootstrap()` instancia el runtime (binding + keyring) y ejecuta `LicenseRuntime::reload_from_storage()`.
2. `reload_from_storage()` consulta SQLite (`license` table).  
   - Si encuentra bytes: `evaluate_license_bytes()` verifica formato, firma y binding.  
   - Se registra `RuntimeInfo` con estado `Active`, `Expired`, `NotYetValid`, `DeviceMismatch` o `Invalid`.
3. Si no existe licencia, se marca `Missing`.
4. La caché (`LicenseState`) solo retiene payload cuando el estado es `Active`.

## Estados expuestos
`RuntimeInfo.status` diferencia explícitamente:

| Estado | Significado |
| --- | --- |
| `Active` | Licencia vigente y ligada al dispositivo. |
| `Missing` | No hay licencia instalada o se removió. |
| `Invalid` | Formato/firma corrupta; no se borra el blob pero el cache queda vacío. |
| `Expired` | Ventana temporal vencida. |
| `NotYetValid` | Licencia aún no vigente (respeta `max_clock_skew`). |
| `DeviceMismatch` | Binding no coincide con el hash del dispositivo actual. |

## Mapeo de `ensure_active()`
`LicenseRuntime::ensure_active()` revisa `RuntimeInfo` y sólo permite continuar cuando el estado es `Active`. Para el resto, retorna:

| Estado | Código de error |
| --- | --- |
| `Missing` | `LicenseRequired` |
| `Invalid` | `Invalid` (mensaje del último error si existe) |
| `Expired` | `Expired` |
| `NotYetValid` | `NotYetValid` |
| `DeviceMismatch` | `DeviceMismatch` |

Si el estado es `Active`, se ejecuta la verificación temporal adicional usando `LicenseState`. Si en ese punto la licencia ya expiró o aún no es válida, el runtime actualiza automáticamente el `RuntimeInfo` al nuevo estado correspondiente antes de devolver el error.

## Guard/enforcement
- `Db::require_license()` ahora recibe el runtime y simplemente llama `ensure_active()`.  
  No existe lógica duplicada de licencias dentro de `Db`.
- Las rutas protegidas (todas las que invocan `db.require_license()`) quedan bloqueadas cuando el runtime reporta un estado distinto de `Active`.
- `LicenseGate` / `LicensePanel` pueden seguir consultando `license_status`, que usa `RuntimeInfo` para mostrar el detalle.

## Estado local frente a licencias inválidas
- Cuando `bootstrap` detecta bytes inválidos/corruptos, limpia únicamente la caché activa (`LicenseState`) y deja `RuntimeInfo.status = Invalid`, preservando `last_error`.
- Si la licencia persiste pero su binding no coincide, el estado pasa a `DeviceMismatch` sin borrar el archivo.
- `remove_license` usa `runtime.mark_license_missing()` para garantizar que cache y resumen vuelvan a `Missing`.

## Qué aún no se activa
- Snapshots/history siguen preparados pero no intervienen en el enforcement.
- No se agregó UI adicional ni nuevos comandos.
- No se adelantó la rotación de llaves ni modificaciones al contrato `.req`.

## Pruebas agregadas
- `bootstrap_valid_license_updates_runtime_and_guard`: verifica que un blob válido (simulando lectura al arranque) deja `RuntimeInfo` + `LicenseState` en `Active` y `Db::require_license()` permite avanzar.
- `bootstrap_invalid_license_blocks_guard`: garantiza que un blob inválido no marca la licencia como activa y el guard lanza error `Invalid`.
- Se actualizaron las pruebas del runtime para usar `update_cache()` y mantener el resumen consistente.

## Próximos pasos sugeridos
1. Integrar snapshots resilientes en el runtime antes de endurecer enforcement en dispositivos multiusuario.
2. Conectar la UI (`LicenseGate` / `LicensePanel`) al resumen para mostrar los nuevos estados diferenciados.
3. Planear la fase de hardening (Fase 6) con smoke tests que contemplen corrupción de archivos y rotación de llaves públicas.
