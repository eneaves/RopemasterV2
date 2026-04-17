# Licenciamiento Offline — Fase 6 (Hardening y Readiness)

## Resumen ejecutivo
- Se completó la verificación final del flujo de licencias con smoke tests automatizados, simulaciones controladas y procedimientos documentados.
- Los errores críticos (licencia faltante, corrupta, expirada o de otro dispositivo) no dejan el runtime en estado inconsistente; el guard continúa bloqueando según lo esperado.
- Se documentó un plan de troubleshooting y un checklist de readiness para facilitar la operación interna y detectar riesgos antes de un rollout más amplio.

## Alcance
- Cobertura del flujo completo: arranque sin licencia → generación `.req` → importación `.lic` → validación al arranque → eliminación.
- Validación de estados `Missing`, `Active`, `Invalid`, `Expired`, `NotYetValid`, `DeviceMismatch`.
- Documentación de casos borde y plan de respuesta para soporte interno.

## Smoke tests
Clasificación:
- **Ejecutado**: se corrió realmente (automatizado vía `cargo test`/`npx vitest`).
- **Simulado / controlado**: requiere manipular payloads o SQLite (QA) fuera del flujo habitual; se ejecutó usando helpers de test.
- **Documentado**: guía pendiente para QA manual; no se ejecutó en esta iteración.

| # | Escenario | Clasificación | Método / comando | Resultado |
|---|-----------|---------------|------------------|-----------|
| 1 | Arranque sin licencia (`Missing`) | Ejecutado | `cargo test license::runtime::service::tests::ensure_active_without_cache_fails` | El runtime marca `Missing` y el guard devuelve `LicenseRequired`. |
| 2 | Generación y exportación de `.req` | Ejecutado | `cargo test license::runtime::service::tests::generates_request_roundtrip` | Request serializa y re-parsea con `REQUEST_VER` vigente. |
| 3 | Importación / instalación de `.lic` válida | Ejecutado | `cargo test tests::bootstrap_valid_license_updates_runtime_and_guard` | La caché queda `Active`, `ensure_active` permite operaciones. |
| 4 | Arranque con licencia válida persistida | Ejecutado | Mismo test que #3 + `Db::require_license()` | Guard desbloquea `health_check`/`list_series`. |
| 5 | Arranque con licencia corrupta / inválida | Ejecutado | `cargo test tests::bootstrap_invalid_license_blocks_guard` | Runtime pasa a `Invalid`, guard muestra error y no deja cache residual. |
| 6 | Licencia expirada | Simulado / controlado | `cargo test license::validator::tests::expired_license_is_rejected` (payload forzado) | `LicenseRuntimeStatus::Expired`; documentado como simulación QA (no flujo real del generador). |
| 7 | Licencia `device_mismatch` | Simulado / controlado | `cargo test license::validator::tests::device_mismatch_is_rejected` | Estado `DeviceMismatch`, guard rechaza. Simulación controlada. |
| 8 | Licencia `not_yet_valid` | Documentado | QA puede emitir licencia con `not_before` futuro o editar payload en SQLite (ver sección Troubleshooting). No se ejecutó en esta iteración. |
| 9 | Eliminación manual de licencia | Documentado | UI (`LicensePanel` → “Eliminar licencia”) o comando `remove_license`. QA plan descrito; no se ejecutó en esta iteración. |
|10 | `LicenseGate` reaccionando a cada estado | Documentado | Tabla de mensajes + instrucciones de validación visual usando estados forzados; no se ejecutó en esta iteración. |
|11 | API/UI helpers | Ejecutado | `npx vitest run src/lib/license-ui.test.ts src/lib/api.test.ts` | Mapping de copy/errores consistente. |

> Nota sobre simulaciones: Los estados `expired`, `device_mismatch` y otros se reprodujeron usando payloads construidos en tests / helpers (`LicenseRuntime::apply_stored_license_for_test`) o modificando temporalmente el registro en SQLite. Esto es un ejercicio de QA controlado, no un flujo normal del licgenerator.

## Casos borde revisados
- **Falta de licencia / archivos borrados**: `LicenseRuntime::reload_from_storage` marca `Missing` y limpia caché (test #1). Resolución: reinstalar `.lic`.
- **Archivo `.lic` corrupto / firma inválida**: `license_core::parse_license_bytes` o `verify_license` fallan → estado `Invalid`, guard bloquea. Resolución: solicitar reemisión y reinstalar.
- **`DeviceMismatch`**: payload con `allowed_device_hash` distinto → estado naranja en UI; se debe generar un `.req` desde el equipo correcto y pedir una licencia nueva (simulado en test #7).
- **Licencia expirada**: `now - skew > not_after` → estado rojo, guard bloquea. QA puede simular editando `not_after` (ver Troubleshooting).
- **Licencia `NotYetValid`**: ocurre si `now + skew < not_before`; documentado con pasos para QA (no se ejecutó).
- **Errores de IO**: `storage::persist_license_files` / `write_atomic` retornan `Io`; UI muestra mensaje “Error de lectura/escritura”. Resolución: revisar permisos y reintentar.
- **Regenerar `.req`**: recomendado al cambiar plan/dispositivo o si se sospecha de mismatch. La UI ofrece botón con spinner + toast.

## Troubleshooting / procedimientos
1. **Missing**: ejecutar `remove_license` (opcional), regenerar `.req`, importar nueva `.lic`.
2. **Invalid**: borrar `licenses/installed/current.lic`, reimportar `.lic` emitida nuevamente; revisar logs `LicenseRuntime`.
3. **DeviceMismatch**: confirmar `device_hash` mostrado en UI vs `allowed_device_hash` de la licencia (UI ya lo expone); regenerar `.req` desde el equipo correcto.
4. **Expired**: pedir al generador nueva licencia; eliminar la caducada para evitar confusiones.
5. **NotYetValid**: verificar fecha/hora del sistema; si es correcto, solicitar licencia con ventana ajustada. QA puede simular editando la tabla `license` y seteando `raw_bytes` con `not_before` futuro (documentado, no ejecutado).
6. **Archivo corrupto**: si `license_status` → `Invalid`, eliminar y reinstalar. Guard asegura que no quede cache residual.

## Checklist de readiness
| Ítem | Estado |
| --- | --- |
| Device binding persiste en `installation.json` y recupera `device_hash` | ✅ (tests `device_binding`) |
| `LicenseRuntime` carga/valida automáticamente al arranque | ✅ (tests #3-5) |
| Guard (`Db::require_license`) bloquea con mapping exacto | ✅ |
| UI muestra estados + acciones básicas (export/import/remove/refresh) | ✅ (ver Fase 5) |
| Manejo de licencias corruptas / mismatch | ✅ (tests #5 y #7) |
| Manejo de `NotYetValid` probado | ⚠️ Documentado, falta ejercicio manual |
| Snapshots/history para recuperación | ⚠️ Pendiente (diseñado pero no activado) |
| Automatización e2e (CI/Playwright) | ⚠️ No implementado |

## Riesgos y deuda técnica
- **Contratos `.req/.lic`**: Opción A sigue vigente; incorporar `installation_id`/`installation_pubkey` requiere coordinación con el generador (bloqueante para rollout amplio).
- **Snapshots**: aún no se usan para recuperación ante corrupción severa; riesgo moderado si el archivo `.lic` se daña frecuentemente.
- **NotYetValid**: no se validó manualmente en esta iteración; dependerá de QA antes de liberar a clientes.
- **Automatización UI**: falta test end-to-end que combine runtime + frontend.
- **Rotación de llaves**: el keyring es estático; se requiere plan para rotación futura si se compromete la llave pública.

## Evaluación final
- **Uso interno controlado**: **APTO.** Los flujos críticos fueron ejercitados/automatizados y la documentación permite operar con un equipo reducido.
- **Rollout más amplio**: **NO APTO todavía.** Se requiere completar soporte formal para `installation_id`/`installation_pubkey`, hardening de snapshots, pruebas manuales de `NotYetValid` y automatización e2e.
- **Bloqueantes principales**:
  1. Falta soporte contractual actualizado (`installation_id`/`installation_pubkey`).
  2. Sin snapshots/historial para recuperación automática.
  3. Sin validación manual de ventanas `NotYetValid`.
  4. Sin suite e2e que combine UI + runtime.
- **Deuda futura**:
  - Implementar snapshots/state dir con verificación periódica.
  - Añadir monitoreo/telemetría ligera para detectar licencias cercanas a expirar.
  - Automatizar flujo `.req` → generador → `.lic` → importación en CI.
  - Preparar UI/UX para cambios de contrato en Fase 2 (cuando se incluya `installation_id`/`installation_pubkey`).
