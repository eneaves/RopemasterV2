# Auditoría de seguridad – cliente Roping Manager

## 1. Resumen ejecutivo
- El sistema de licencias se apoya por completo en el runtime local (`LicenseRuntime`) y en un guard central (`Db::require_license`), lo cual bloquea a usuarios casuales que solo interactúan con la UI.
- El modelo se debilita frente a atacantes con acceso al filesystem: `installation.json` y los archivos `.lic` son editables/copiar-pegar, así que la vinculación a dispositivo es puramente documental.
- Atacantes con capacidad de reversing tienen un camino directo para parchear el binario o recompilar el cliente sin guard, porque no existe ningún mecanismo de anti-tamper o verificación remota.
- Conclusión: la seguridad efectiva del cliente es baja-moderada; suficiente para usuarios honestos, pero insuficiente para evitar clonación deliberada de licencias u omnibypass local.

## 2. Arquitectura de seguridad actual
- **Runtime**: `LicenseRuntime` administra binding, keyring y caché; expone `ensure_active`, `summary`, instalación y generación de requests (`src-tauri/src/license/runtime/service.rs:18-210`).
- **Guard**: `Db::require_license` delega en el runtime y es invocado manualmente en cada comando sensible (`src-tauri/src/lib.rs:26-34` y llamados en cada handler como `list_series` en `src-tauri/src/lib.rs:140-149`).
- **Bootstrap**: durante `tauri::Builder::setup` se crea el pool SQLite, se corre `license::bootstrap`, y se registra `Db`, `LicenseRuntime` y `LicenseState` para los comandos (`src-tauri/src/lib.rs:3562-3595` + `src-tauri/src/license/mod.rs:116-125`).
- **Device binding**: `DeviceBindingStore` persiste `installation_id` y un `device_hash` aleatorio de 32 bytes dentro de `app_config_dir/device/installation.json` sin firma ni cifrado (`src-tauri/src/license/runtime/device_binding.rs:35-175`).
- **Almacenamiento**: la licencia instalada vive duplicada en SQLite (`license` table) y en archivos `.lic` bajo `app_config_dir/licenses`, también sin protección (`src-tauri/src/license/storage.rs:19-133`).
- **Comandos Tauri**: las operaciones de licencia (`get_device_hash`, `generate_license_request`, `install_license`, `license_status`, `remove_license`) operan exclusivamente contra el runtime y storage interno (`src-tauri/src/license/commands.rs:77-212`).
- **Frontend**: `LicenseProvider` consume `license_status`, bloquea toda la UI con `LicenseGate` mientras `isActive` sea falso (`src/providers/LicenseProvider.tsx:8-55`, `src/App.tsx:35-65`), pero el enforcement real está en el backend.

## 3. Superficies de ataque
- **UI/JS (atacante casual)**: sin licencia el frontend expone únicamente `LicenseGate`; incluso si el usuario abre devtools e invoca comandos, el guard backend niega el acceso.
- **Filesystem (atacante con acceso local)**: directorios `device/` y `licenses/` en `app_config_dir` contienen toda la identidad del dispositivo y los blobs firmados; copiar o editar esos archivos altera el estado sin validaciones adicionales.
- **Runtime/guard (atacante reversing)**: `Db::require_license` es una función pequeña y todas las verificaciones se ejecutan en el cliente; parchear el binario o recompilar con la verificación removida desbloquea todo.
- **Contrato `.req/.lic`**: el request solo serializa `device_hash`, plan, nonce y metadatos (`crates/license_core/src/request.rs:7-17`), así que el generador no distingue clones.
- **Llave pública fija**: `public_key_dev.der` embebido en el binario (`src-tauri/src/license/runtime/keyring.rs:13-34`); un leak de la private key permitiría emitir licencias válidas sin restricción.

## 4. Vulnerabilidades concretas
| ID | Severidad | Atacante | Descripción |
| --- | --- | --- | --- |
| V1 | Alta | Filesystem | La vinculación de dispositivo es un JSON editable ubicado en `app_config_dir/device/installation.json`; `DeviceBindingStore` solo hace `serde_json::from_slice` y devuelve el `device_hash` sin ninguna firma o validación adicional (`src-tauri/src/license/runtime/device_binding.rs:35-175`). Como `validator::runtime_state` únicamente compara ese hash con el campo `allowed_device_hash` del payload (`src-tauri/src/license/validator.rs:64-83`), basta con copiar/editar el archivo y transportar también `licenses/current.lic` (`src-tauri/src/license/storage.rs:52-69`) para clonar una licencia en otra máquina. |
| V2 | Alta | Filesystem | El contrato de request no incluye `installation_id`, `installation_pubkey` ni prueba criptográfica de la identidad local (`crates/license_core/src/request.rs:7-17`), y la documentación reconoce que dichos campos no se implementaron (`docs/app-license-phase-6-hardening-and-readiness.md:65-78`). Por lo tanto, el generador no tiene forma de detectar solicitudes fabricadas o reutilizadas desde clones de `installation.json`, lo que facilita compartir licencias. |
| V3 | Media | Reversing | Todo el enforcement vive en el cliente: `Db::require_license` llama `LicenseRuntime::ensure_active` y decide en base al caché local (`src-tauri/src/lib.rs:26-34`, `src-tauri/src/license/runtime/service.rs:135-158`). No existe anti-tamper, ofuscación ni validación servidor→cliente, así que un atacante puede parchear el binario (o recompilar el proyecto) para que `ensure_active` siempre devuelva `Ok`, eliminando el guard. |
| V4 | Media | Casual/Dev | El guard se invoca manualmente en cada comando Tauri; no hay macro o middleware obligatorio. Un endpoint nuevo que olvide `db.require_license()?;` quedaría abierto y puede ser llamado directamente vía `invoke` aunque la UI esté bloqueada (`src-tauri/src/lib.rs:140-2205`). Actualmente todos los comandos la llaman, pero es una deuda estructural. |
| V5 | Media | Reversing | Solo existe una llave pública embebida (`src-tauri/src/license/runtime/keyring.rs:13-34`) y no hay estrategia de rotación ni de revocación (documentado como deuda en `docs/app-license-phase-6-hardening-and-readiness.md:64-77`). Comprometer la clave privada (p.ej. porque también se distribuye en herramientas internas) permitiría generar licencias válidas ilimitadas. |

## 5. Vectores de bypass reales
1. **Clonación completa (filesystem)**: copiar `~/Library/Application Support/<app>/device/installation.json` y `.../licenses/` desde un equipo licenciado a otro, reiniciar la app y ambos quedan `Active` porque el runtime confía ciegamente en esos archivos.
2. **Imitación puntual (filesystem)**: abrir `installation.json`, reemplazar `device_hash_hex` por el hash incluido dentro de una `.lic` ajena (se puede extraer vía `license_status` o decodificando CBOR), reinstalar la licencia y el runtime la aceptará al coincidir el hash.
3. **Request fabricado (filesystem)**: borrar `installation.json` para forzar que `DeviceBindingStore` emita un nuevo hash, generar `.req`, restaurar después el archivo original y seguir usando la licencia previa; el generador no puede distinguir qué hash es legítimo.
4. **Patch del guard (reversing)**: modificar el binario (o construir desde código fuente) para que `Db::require_license` devuelva siempre `Ok(())` o para que `validator::runtime_state` ignore `DeviceMismatch`; sin comprobaciones externas, el backend aceptará todas las operaciones.
5. **Forzado de estado en memoria (reversing avanzado)**: un atacante puede hookear el proceso y escribir en `LicenseState` (Arc<RwLock>) para inyectar un `LicenseCache` válido en caliente, logrando que `ensure_active` devuelva `Ok` sin siquiera instalar una licencia real (`src-tauri/src/license/mod.rs:20-84`).

## 6. Qué partes están bien implementadas
- Validación criptográfica firme: `license_core::verify_license` usa ed25519 y valida `app_id`, fechas y `device_hash` (`src-tauri/src/license/validator.rs:24-83`).
- Guardado atómico y cache coherente: los blobs y archivos se escriben con `write_atomic`, evitando corrupciones parciales (`src-tauri/src/license/storage.rs:52-69`).
- UI subordinada al backend: aun con `LicenseGate`, todos los comandos sensibles dependen del guard backend, así que no hay enforcement exclusivo en React (`src/App.tsx:35-65`).
- Manejo explícito de estados (`Missing`, `Expired`, `DeviceMismatch`, etc.) que facilita diagnósticos (`src-tauri/src/license/runtime/service.rs:30-210`).

## 7. Debilidades críticas
- Dependencia absoluta de archivos locales sin integridad: cualquier usuario avanzado puede copiar/editar binding y licencias para duplicar instalaciones.
- Ausencia de identidad criptográfica; no existe `installation_pubkey` ni atestación en los requests, de modo que el backend no detecta clones.
- Enfoque 100 % cliente: basta modificar el binario para desactivar la protección; no hay verificación con un servicio central.
- Llave única embebida, sin rotación; un leak sería catastrófico y no existen mecanismos de revocación en el cliente.

## 8. Recomendaciones (sin implementar aún)
1. **Capa de identidad firme**: generar un par Ed25519 por instalación, firmar `installation.json` y enviar `installation_id` + `installation_pubkey` dentro del `.req`; validar firmas al leer el binding. Esto haría inviable editar el JSON manualmente.
2. **Entrelazar binding con hardware ligero**: aunque no haya un driver, mezclar el hash con atributos estáticos (serial de disco, CPUID) y guardar solo hashes derivados para que copiar archivos no sea suficiente; documentar los límites offline.
3. **Fortalecer el guard**: crear un wrapper/atributo que obligue a pasar por `require_license` (p.ej. macro procedural) para reducir el riesgo de endpoints sin protección.
4. **Plan de rotación de llaves**: introducir un keyring con múltiples claves públicas y metadata `key_id` en las licencias, permitiendo revocar la actual y distribuir una nueva.
5. **Medidas de detección**: al menos registrar hashes de instalación (firmados) cuando se genera `.req` y correlacionarlos en el generador para identificar clones evidentes.
6. **Tamper básico**: añadir checksums del binario o validación contra un backend ligero (por ejemplo, comparar `device_hash` con un registro de instalaciones) para elevar el costo de parcheo.

## 9. Qué no vale la pena proteger (costo vs beneficio)
- Seguridad perfecta offline/no distribuible es inalcanzable; invertir en ofuscaciones pesadas o drivers kernel sólo elevaría costos sin impedir a un atacante determinado.
- Intentar bloquear por completo el cambio de fecha/hora del sistema no es realista para una app de escritorio sin privilegios elevados.
- Cifrar los archivos `.lic` localmente no añade valor mientras el binding siga siendo editable; primero conviene cerrar ese vector.

## 10. Prioridad de mitigación
1. **Cerrar el hueco de binding editable (V1 + V2)**: implementar firma/clave por instalación y extender el contrato `.req/.lic` para que el generador pueda validar identidad.
2. **Diseñar/ejecutar estrategia de rotación de llaves (V5)** para poder revocar el keypair actual en caso de leak.
3. **Automatizar el guard**: crear tooling que garantice que todo comando pase por `require_license`, evitando errores futuros (V4).
4. **Agregar controles anti-tamper ligeros** (telemetría o validación remota) para elevar el costo de patching (V3).
5. **Monitorear y detectar clonaciones** registrando installs/licencias emitidas y comparando `installation_id` futuros.
