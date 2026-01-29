import { Sidebar } from './components/Sidebar'
import { Dashboard } from './components/Dashboard'
import { RopersManagement } from './components/RopersManagement'
import { PayoffsManagement } from './components/PayoffsManagement'
import { EventsCalendar } from './components/EventsCalendar'
import { TeamsManagement } from './components/TeamsManagement'
import { ResultsManagement } from './components/ResultsManagement'
import { ExportManagement } from './components/ExportManagement'
import { SettingsManagement } from './components/SettingsManagement'
import { CaptureManagement } from './components/CaptureManagement'
import { ActivityLogView } from './components/ActivityLogView'
import { Toaster } from './components/ui/sonner'
import { useState } from 'react'

/**
 * Roping Manager — Aplicación principal
 *
 * Sistema de gestión de competencias de team roping con flujo completo:
 * Dashboard (Series) → Eventos → Workspace
 *
 * Navegación:
 * - dashboard: Vista principal
 * - eventos: Calendario de eventos
 * - equipos: Gestión de equipos
 * - ropers: Gestión de competidores
 * - captura: Captura de tiempos
 * - resultados: Visualización de resultados
 * - payoffs: Cálculo de premios
 * - exportar: Exportación de reportes
 * - settings: Configuración general
 */
export default function App() {
  const [activeMenuItem, setActiveMenuItem] = useState('dashboard')

  // Maneja la navegación entre vistas principales
  const handleMenuItemClick = (item: string) => {
    setActiveMenuItem(item)
  }

  // Renderiza el contenido según la vista activa
  const renderContent = () => {
    switch (activeMenuItem) {
      case 'dashboard':
        return <Dashboard onNavigate={handleMenuItemClick} />
      case 'eventos':
        return <EventsCalendar />
      case 'equipos':
        return <TeamsManagement />
      case 'ropers':
        return <RopersManagement />
      case 'captura':
        return <CaptureManagement />
      case 'resultados':
        return <ResultsManagement />
      case 'payoffs':
        return <PayoffsManagement />
      case 'exportar':
        return <ExportManagement />
      case 'settings':
        return <SettingsManagement />
      case 'activity':
        return <ActivityLogView onBack={() => setActiveMenuItem('dashboard')} />
      default:
        return <Dashboard onNavigate={handleMenuItemClick} />
    }
  }

  return (
    <div className="h-screen w-screen flex bg-background text-foreground">
      {/* Sidebar de navegación principal */}
      <Sidebar activeItem={activeMenuItem} onItemClick={handleMenuItemClick} />

  {/* Contenido principal: cada vista administra su propio scroll para centro y panel derecho */}
  <main className="flex-1 min-h-0">{renderContent()}</main>

      {/* Sistema de notificaciones toast */}
      <Toaster richColors position="top-right" />
    </div>
  )
}
