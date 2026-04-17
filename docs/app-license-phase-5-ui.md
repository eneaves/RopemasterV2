# Licenciamiento Offline — Fase 5 (UI mínima)

## Resumen ejecutivo
- La app ya expone un panel de licencias con estado en tiempo real, acciones para exportar `.req`, importar `.lic`, remover licencias y refrescar el estado.
- `LicenseRuntime` permanece como fuente de verdad; la UI solo consume los comandos existentes (`license_status`, `generate_license_request`, `install_license`, `remove_license`, `get_device_hash`).
- `LicenseGate` y `LicensePanel` muestran mensajes consistentes por cada estado (`Active`, `Missing`, `Expired`, `NotYetValid`, `DeviceMismatch`, `Invalid`) y reflejan los errores del backend mediante un mapping estable `code -> mensaje`.

## Componentes actualizados
- `LicenseProvider`: ahora diferencia `loading` (primer arranque) de `refreshing` (acciones manuales), exponiendo ambos flags a la UI.
- `LicensePanel`: muestra badges, mensajes y acciones con loaders independientes (refresh, generar request, instalar, remover); los errores usan `mapCommandErrorToCopy`.
- `LicenseGate`: reutiliza el mapping de estados y muestra spinners al reintentar la verificación.
- `SettingsManagement`: refleja el mismo estado/resumen y ofrece botón de refresh con loader.

## Estados mostrados y copy principal
| Estado (`status`) | Badge | Mensaje principal |
| --- | --- | --- |
| `active` | Activa (verde) | “Licencia activa. Expira el …” |
| `missing` | Sin licencia (gris) | “Instala una licencia válida…” |
| `expired` | Expirada (rojo) | “Licencia expirada desde …” |
| `not_yet_valid` | Pendiente (amarillo) | “Será válida a partir de …” |
| `device_mismatch` | Otro dispositivo (naranja) | “La licencia pertenece a otro dispositivo (hash …)” |
| `invalid` | Licencia inválida (rosa) | “El archivo instalado es inválido/corrupto.” |

## Acciones disponibles en el panel
- **Actualizar estado** (`license_status`): botón con spinner (`Loader2`) atado a `refresh()` del provider.
- **Generar `.req`** (`generate_license_request`): respeta plan y hint opcional; botón muestra progreso independiente.
- **Instalar `.lic`** (`install_license`): abre diálogo del sistema, muestra spinner durante importación y actualiza el estado local con la respuesta del backend.
- **Eliminar licencia** (`remove_license` + `license_status` subsecuente): requiere confirmación, muestra loader y refresca el estado.
- **Copiar device hash** (`get_device_hash`): expone botón “Copiar” con toast.

## Errores y mapping
Los errores del backend se traducen mediante `mapCommandErrorToCopy`:

| Código | Título mostrado | Descripción |
| --- | --- | --- |
| `LicenseRequired` | Licencia requerida | Instala una licencia válida para continuar. |
| `Expired` | Licencia expirada | Usa nueva licencia vigente. |
| `NotYetValid` | Licencia aún no válida | Revisa fecha/ventana. |
| `DeviceMismatch` | Licencia de otro dispositivo | Archivo pertenece a otra instalación. |
| `Invalid` / `SignatureFailed` / `Parse` | Archivo/Firma inválidos | Archivo dañado o alterado. |
| `Io` | Error de lectura/escritura | Problema de disco/permisos. |
| Cualquier otro | “Operación fallida” + detalle original como descripción. |

## LicenseGate por estado
| Estado | Comportamiento |
| --- | --- |
| `Active` | Gate muestra mensaje “Licencia verificada” pero habitualmente no se renderiza porque el guard permite el acceso. |
| `Missing` | Muestra alerta y CTA para instalar licencia; bloqueo total. |
| `Expired` | Mensaje rojo indicando renovación obligatoria; bloqueo. |
| `NotYetValid` | Mensaje amarillo con instrucción de revisar fecha; bloqueo. |
| `DeviceMismatch` | Mensaje naranja indicando que la licencia pertenece a otro dispositivo; bloqueo. |
| `Invalid` | Mensaje rosa indicando corrupción/archivo inválido; bloqueo. |

## Estados de carga visuales
- **Refresh**: `LicenseProvider.refreshing` controla el spinner en `LicenseGate`, `SettingsManagement` y `LicensePanel`.
- **Generar `.req`**: botón muestra loader y texto “Generando…”.
- **Importar `.lic`**: botón muestra loader “Instalando…”.
- **Eliminar licencia**: botón muestra loader “Eliminando…”.

## Pruebas
1. **Vitest**: `src/lib/license-ui.test.ts` verifica badges, mensajes y mapping de errores; `src/lib/api.test.ts` continúa pasando.
2. **Manual**:
   - Generar `.req` (seleccionar destino) → toast de éxito + archivo.
   - Instalar `.lic` válido (fixture emitido por generador) → estado cambia a `Active`.
   - Remover licencia → estado vuelve a “Sin licencia”.
   - Probar archivo inválido → toast muestra “Licencia inválida” y detalle.
   - Forzar `DeviceMismatch` (archivo de otro equipo) → UI muestra estado naranja y bloqueo en `LicenseGate`.

## Pendientes/fuera de alcance
- No se añadió UI para snapshots/history ni para múltiples licencias.
- Falta integrar notificaciones en tiempo real o eventos para que otras vistas se actualicen automáticamente (hoy dependen del botón de refresh o del provider).
- Hardening y pruebas e2e se reservan para Fase 6.
