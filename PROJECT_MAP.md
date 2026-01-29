# Mapeo del Proyecto: Roping Manager Tauri

Este documento describe la arquitectura, estructura y componentes del sistema Roping Manager, una aplicación de escritorio construida con Tauri (Rust) y React.

## 1. Visión General

*   **Frontend:** React + Vite + TypeScript + Tailwind CSS.
*   **Backend:** Tauri (Rust) + SQLite (sqlx).
*   **Arquitectura:** Aplicación de escritorio local donde el frontend se comunica con el proceso backend de Rust mediante IPC (Tauri Commands).
*   **Base de Datos:** SQLite local (`roping_manager.db`) gestionada con migraciones `sqlx`.

## 2. Estructura de Archivos Clave

```
/
├── src/                        # Frontend (React)
│   ├── components/             # Componentes de UI y Vistas
│   │   ├── ui/                 # Componentes base (shadcn/ui)
│   │   ├── Dashboard.tsx       # Vista principal
│   │   ├── EventsCalendar.tsx  # Vista de calendario
│   │   ├── CaptureManagement.tsx # Vista de captura de tiempos
│   │   └── ...
│   ├── lib/
│   │   ├── api.ts              # Cliente API (wrappers de invoke)
│   │   └── utils.ts            # Utilidades generales
│   ├── App.tsx                 # Enrutador principal y Layout
│   └── types.ts                # Definiciones de tipos TypeScript
├── src-tauri/                  # Backend (Rust)
│   ├── src/
│   │   ├── lib.rs              # Lógica principal y registro de comandos
│   │   └── main.rs             # Punto de entrada
│   ├── migrations/             # Scripts SQL de migración
│   ├── tauri.conf.json         # Configuración de Tauri
│   └── Cargo.toml              # Dependencias de Rust
└── package.json                # Dependencias de Node/React
```

## 3. Backend (Rust/Tauri)

El backend expone una serie de comandos invocables desde el frontend y gestiona la persistencia en SQLite.

### Base de Datos (SQLite)

El esquema se define en `src-tauri/migrations/`.

*   **`series`**: Agrupación de eventos (Temporadas).
*   **`event`**: Competencias individuales.
    *   Estados: `upcoming`, `active`, `completed`, `locked`, `archived`.
*   **`roper`**: Competidores (Headers/Heelers).
    *   Niveles: `pro`, `amateur`, `principiante`.
*   **`team`**: Parejas de ropers para un evento.
*   **`draw`**: Asignación de equipos a posiciones en rondas.
*   **`run`**: Ejecución de una ronda por un equipo (tiempos, penalizaciones).
*   **`payoff_rule` / `payoff`**: Reglas y distribución de premios.
*   **Tablas de Sistema**: `app_user`, `role`, `audit_log` (Infraestructura de identidad/auditoría).

### Comandos API (Tauri Commands)

Definidos en `src-tauri/src/lib.rs`.

| Categoría | Comando | Descripción |
| :--- | :--- | :--- |
| **Sistema** | `health_check` | Verifica conexión a BD. |
| **Series** | `list_series`, `create_series`, `update_series`, `delete_series` | CRUD de series. |
| **Eventos** | `list_events`, `create_event`, `update_event`, `delete_event` | CRUD de eventos. |
| | `duplicate_event`, `lock_event`, `update_event_status` | Acciones específicas. |
| **Equipos** | `list_teams`, `create_team`, `update_team`, `delete_team` | Gestión de equipos. |
| | `hard_delete_teams_for_event` | Limpieza masiva. |
| **Ropers** | `list_ropers`, `create_roper`, `update_roper`, `delete_roper` | Gestión de competidores. |
| **Captura** | `save_run`, `get_runs` | Registro de tiempos y resultados. |
| | `generate_draw`, `get_draw` | Generación de orden de salida. |
| **Resultados**| `get_standings` | Cálculo de posiciones y promedios. |
| **Payoffs** | `list_payoff_rules`, `delete_payoff_rule` | Gestión de reglas de pago. |

## 4. Frontend (React)

La aplicación es una SPA (Single Page Application) gestionada por `App.tsx` que renderiza componentes basados en el estado `activeMenuItem`.

### Vistas Principales (`src/components/`)

1.  **Dashboard (`Dashboard.tsx`)**: Resumen de series y actividad reciente.
2.  **Eventos (`EventsCalendar.tsx`)**: Calendario y lista de eventos. Permite crear/editar eventos.
3.  **Equipos (`TeamsManagement.tsx`)**: Gestión de inscripciones para un evento seleccionado.
4.  **Ropers (`RopersManagement.tsx`)**: Directorio global de competidores.
5.  **Captura (`CaptureManagement.tsx`)**: Interfaz principal durante el evento para registrar tiempos.
6.  **Resultados (`ResultsManagement.tsx`)**: Tablas de posiciones (Standings).
7.  **Payoffs (`PayoffsManagement.tsx`)**: Configuración y cálculo de premios.

### Integración (`src/lib/api.ts`)

Este archivo actúa como puente. Cada función exportada llama a `invoke('command_name', { args })`.
*   Maneja la conversión de tipos básicos.
*   Normaliza estados (ej. `draft` -> `upcoming`) antes de enviar al backend.

## 5. Flujos de Datos Clave

### Flujo de Competencia
1.  **Creación:** Usuario crea una **Serie** y luego un **Evento** dentro de ella.
2.  **Inscripción:** En la vista **Equipos**, se seleccionan **Ropers** (Header/Heeler) para crear **Teams**.
3.  **Sorteo (Draw):** Se ejecuta `generate_draw` para asignar el orden de salida.
4.  **Captura:** En **Captura**, el operador ingresa tiempos (`save_run`) para cada equipo/ronda.
5.  **Resultados:** El sistema calcula `get_standings` en tiempo real basado en los `run` completados.

### Política de Datos
*   **Soft Delete:** La mayoría de los borrados (`delete_event`, `delete_roper`) solo marcan registros como inactivos (`is_deleted=1` o `is_active=0`) para preservar integridad histórica.
*   **Bloqueo:** Los eventos pueden estar `locked`, impidiendo modificaciones estructurales (borrar equipos, cambiar configuración) una vez iniciados.
