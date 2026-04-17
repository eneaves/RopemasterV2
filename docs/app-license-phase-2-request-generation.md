# Licenciamiento Offline — Fase 2 (Generación de `.req`)

## Resumen ejecutivo
- Se adopta la **Opción A (compatibilidad conservadora)**: el `.req` sigue usando el contrato actual de `LicenseRequest`/`REQUEST_VER`.
- `LicenseRuntime` ahora genera el request completo usando el binding persistido, serializa, re-parsea y valida antes de escribir cualquier archivo.
- Los comandos Tauri mantienen su firma; internamente solo invocan al runtime y usan los nuevos helpers para exportar los bytes.

## Flujo actualizado de generación/exportación
1. `LicenseRuntime::generate_request_bytes(plan, hint)` arma el request con `device_hash` actual y `DEFAULT_APP_ID`.
2. Se serializa con `license_core::request::request_to_bytes`, se vuelve a parsear y se devuelve la versión normalizada más los bytes.
3. `generate_license_request` escribe primero el archivo en `licenses/requests/<timestamp>-<plan>-<hash>.req` y luego, si corresponde, en la ruta elegida por el usuario.
4. Se devuelve un `LicenseRequestSummaryDto` con los datos normalizados para la UI (hash, nonce y timestamps).

## Datos del runtime vs datos enviados en Fase 2
| Concepto | ¿Existe en runtime? | ¿Viaja en el `.req` actual? | Comentario |
| --- | --- | --- | --- |
| `installation_id` | Sí (persistido en `installation.json`) | No | Se reserva para Fase 3/4; requiere coordinar `REQUEST_VER` con el licgenerator. |
| `installation_pubkey` | Campo reservado (`Option<Vec<u8>>`) | No | Aún no se genera ni se expone para no romper compatibilidad. |
| `device_hash` | Sí | Sí (campo `device_hash`) | Continúa siendo el binding oficial del request. |
| `plan`, `customer_name_hint`, `nonce`, `created_at` | Sí (calculado al vuelo) | Sí | Igual que en el contrato vigente. |
| `app_id` | Sí (constante `DEFAULT_APP_ID`) | Sí | Validación idéntica al licgenerator. |

## Qué falta para incluir `installation_id` / `installation_pubkey`
1. Extender `license_core::request::LicenseRequest` y `REQUEST_VER` para aceptar campos opcionales/compatibles.
2. Actualizar el licgenerator para leer dichos campos y vincular licencias con la instalación/clave pública.
3. Ajustar `LicenseRuntime::generate_request_bytes` para poblarlos y validar que existan.
4. Añadir pruebas cruzadas (cliente + licgenerator) que verifiquen ambos formatos durante la transición.

## Compatibilidad
- El `.req` resultante sigue pasando `license_core::request::parse_request_bytes` sin cambios y es idéntico al que espera el licgenerator actual.
- Las pruebas unitarias garantizan la round-trip (`generate_request_roundtrip`) y que la exportación escribe bytes exactos.

## Próximos pasos sugeridos
1. Coordinar con el licgenerator el cambio de contrato cuando se decida usar `installation_id`/`installation_pubkey`.
2. Añadir snapshots del request/snapshots de instalación para auditoría (Fase 3).
3. Empezar a exponer UI de exportación mejorada una vez que el request extendido esté acordado.
