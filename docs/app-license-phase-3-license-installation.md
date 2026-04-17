# Licenciamiento Offline — Fase 3 (Instalación de `.lic`)

## Resumen ejecutivo
- La instalación de licencias ahora se realiza a través de `LicenseRuntime`, reutilizando el binding del dispositivo y la llave pública embebida.
- Se validan formato, firma y clasificación del payload antes de persistir; sólo las licencias evaluadas como `Active` se aceptan.
- Los comandos públicos (`install_license`) mantienen su firma, pero internamente usan el runtime para asegurar que una licencia rechazada no impacte storage ni cache.

## Condiciones para considerar una licencia instalable (Fase 3)
1. Debe ser parseable por `license_core::parse_license_bytes` (estructura CBOR válida).
2. La firma Ed25519 debe verificarse con la llave pública activa embebida.
3. `validator::evaluate_license` debe clasificarla como `LicenseRuntimeStatus::Active` (app_id correcto, ventana válida, device hash coincidente).
4. Cualquier otro estado (`Expired`, `NotYetValid`, `DeviceMismatch`) se rechaza con error específico y la licencia no se persiste ni actualiza cache.

## Flujo de importación
1. UI/comando entrega ruta o bytes (`LicenseInputPayload`).
2. `LicenseRuntime::evaluate_license_bytes` realiza parse + verificación + runtime policy.
3. Si el estado es `Active`, se persisten los bytes en SQLite + `licenses/installed/current.lic` y se actualiza el cache (`LicenseState`).
4. Se devuelve el DTO con la información normalizada; si ocurre un error, se propaga con códigos consistentes (`Io`, `Parse`, `DeviceMismatch`, etc.) y no se toca storage.

## Validaciones presentes vs. pendientes
| Validación | ¿Incluida en Fase 3? | Comentario |
| --- | --- | --- |
| Formato `.lic` (CBOR + tamaño) | Sí | `license_core::parse_license_bytes`.
| Firma Ed25519 | Sí | Reutiliza el keyring embebido.
| `app_id`, `plan`, ventana, clock skew | Sí | `validator::runtime_state`.
| Binding de dispositivo | Sí | Compara `allowed_device_hash` con el hash actual.
| Snapshots / historial avanzado | No | Se realizará en fases posteriores.
| Validación automática al arranque | No | Sigue pendiente para Fase 4.

## Estados aceptados en esta fase
- Solo licencias con `LicenseRuntimeStatus::Active` se instalan.
- Estados `Expired`, `NotYetValid` y `DeviceMismatch` generan errores específicos y terminan el flujo sin persistencia.

## Pruebas añadidas
- Se añadieron pruebas unitarias que garantizan que una licencia inválida o rechazada no escribe en disco ni actualiza el cache.
- Se cubre la escritura de archivos `.lic` y la validación round-trip del runtime.

## Próximos pasos
1. Integrar snapshots/state dir y validación automática al arranque (Fase 4).
2. Exponer UX adicional (Fase 5) para mostrar estados y errores.
3. Coordinar cambios futuros al contrato `.req` antes de incorporar `installation_id`/`installation_pubkey`.
