import { Navigate, Outlet, Route, Routes, useNavigate } from 'react-router-dom'
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
import { LicenseGate } from './components/LicenseGate'
import { useLicense } from './providers/LicenseProvider'
import { SeriesEventsProvider } from './providers/SeriesEventsProvider'

function ActivityLogPage() {
  const navigate = useNavigate()
  return <ActivityLogView onBack={() => navigate(-1)} />
}

function AppShell() {
  return (
    <div className="h-screen w-screen flex bg-background text-foreground">
      <Sidebar />
      <main className="flex-1 min-h-0 overflow-hidden">
        <Outlet />
      </main>
      <Toaster richColors position="top-right" />
    </div>
  )
}

export default function App() {
  const { isActive } = useLicense()

  if (!isActive) {
    return (
      <>
        <LicenseGate />
        <Toaster richColors position="top-right" />
      </>
    )
  }

  return (
    <SeriesEventsProvider>
      <Routes>
        <Route element={<AppShell />}>
          <Route index element={<Dashboard />} />
          <Route path="/eventos" element={<EventsCalendar />} />
          <Route path="/equipos" element={<TeamsManagement />} />
          <Route path="/ropers" element={<RopersManagement />} />
          <Route path="/captura" element={<CaptureManagement />} />
          <Route path="/resultados" element={<ResultsManagement />} />
          <Route path="/payoffs" element={<PayoffsManagement />} />
          <Route path="/exportar" element={<ExportManagement />} />
          <Route path="/settings" element={<SettingsManagement />} />
          <Route path="/activity" element={<ActivityLogPage />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Route>
      </Routes>
    </SeriesEventsProvider>
  )
}
