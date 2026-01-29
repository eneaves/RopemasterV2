import { Plus, Search } from 'lucide-react'
import { Button } from './ui/button'
import { Input } from './ui/input'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from './ui/dialog'
import { SeriesCard } from './SeriesCard'
import { EventsView } from './EventsView'
import { EventDetails } from './EventDetails'
import { MetricsPanel } from './MetricsPanel'
import { RecentActivity } from './RecentActivity'
import { NewSeriesModal } from './NewSeriesModal'
import { toast } from 'sonner'
import { useState, useEffect, useMemo, useDeferredValue } from 'react'
import { getSeries, createSeries, updateSeries, deleteSeries, getDashboardStats, getRecentActivity } from '@/lib/api'
import type { Series, Event, DashboardStats, AuditLogItem } from '../types'

const initialSeries: Series[] = [
  {
    id: 1,
    name: 'Winter Classic 2025',
    season: 'Season 2025',
    dateRange: 'Jan - Mar',
    eventsCount: 8,
    progress: 75,
    status: 'active',
    description: 'Serie de invierno con competencias semanales',
  },
  {
    id: 2,
    name: 'Summer Shootout',
    season: 'Season 2025',
    dateRange: 'Jun - Aug',
    eventsCount: 12,
    progress: 30,
    status: 'active',
    description: 'Competencia de verano de alto nivel',
  },
  {
    id: 3,
    name: 'Fall Championship',
    season: 'Season 2025',
    dateRange: 'Sep - Nov',
    eventsCount: 6,
    progress: 0,
    status: 'upcoming',
    description: 'Campeonato de otoño regional',
  },
  {
    id: 4,
    name: 'Spring Training Series',
    season: 'Season 2025',
    dateRange: 'Mar - May',
    eventsCount: 10,
    progress: 100,
    status: 'archived',
    description: 'Serie de entrenamiento primavera',
  },
]


interface DashboardProps {
  onNavigate?: (item: string) => void
}

export function Dashboard({ onNavigate }: DashboardProps = {}) {
  const [series, setSeries] = useState<Series[]>(initialSeries)
  const [selectedSeries, setSelectedSeries] = useState<Series | null>(null)
  const [selectedEvent, setSelectedEvent] = useState<Event | null>(null)
  const [isNewSeriesModalOpen, setIsNewSeriesModalOpen] = useState(false)
  const [editingSeries, setEditingSeries] = useState<Series | null>(null)
  const [deleteCandidate, setDeleteCandidate] = useState<Series | null>(null)
  const [searchQuery, setSearchQuery] = useState('')
  const [dashboardStats, setDashboardStats] = useState<DashboardStats | undefined>(undefined)
  const [recentActivity, setRecentActivity] = useState<AuditLogItem[]>([])

  const refreshDashboard = async () => {
    try {
      const [seriesData, statsData, activityData] = await Promise.all([
        getSeries(),
        getDashboardStats(),
        getRecentActivity(20)
      ])
      setSeries((seriesData as any[]).map(mapSeriesRowToSeries))
      setDashboardStats(statsData)
      setRecentActivity(activityData)
    } catch (e) {
      console.error('Failed to refresh dashboard:', e)
    }
  }

  const handleViewSeries = (s: Series) => setSelectedSeries(s)
  const handleBackToSeries = () => {
    setSelectedSeries(null)
    setSelectedEvent(null)
  }
  const handleViewEvent = (e: Event) => setSelectedEvent(e)
  const handleBackToEvents = () => setSelectedEvent(null)

  const handleCreateSeries = async (newSeries: Series) => {
    try {
      const payload: any = {
        name: newSeries.name,
        season: newSeries.season,
        status: (newSeries.status as 'active' | 'upcoming' | 'archived') ?? 'upcoming',
      }

      // si dateRange viene en formato 'YYYY-MM-DD - YYYY-MM-DD' o similar, intentar mapear
      if (newSeries.dateRange) {
        const parts = String(newSeries.dateRange).split(' - ').map((p) => p.trim())
        if (parts[0]) payload.start_date = parts[0]
        if (parts[1]) payload.end_date = parts[1]
      }

      await createSeries(payload)

      // refrescar lista desde backend
      await refreshDashboard()

      toast.success(`Serie "${newSeries.name}" creada exitosamente!`)
      // cerrar modal
      setIsNewSeriesModalOpen(false)
    } catch (e: any) {
      toast.error(e?.toString?.() ?? 'No se pudo crear la serie')
    }
  }

  // Events: moved into EventsView which loads from backend directly

  const handleEditSeries = (seriesId: number) => {
    const s = series.find((x) => x.id === seriesId)
    if (!s) return
    setEditingSeries(s)
    setIsNewSeriesModalOpen(true)
  }

  const handleDuplicateSeries = async (seriesId: number) => {
    const s = series.find((x) => x.id === seriesId)
    if (!s) return
    try {
      const payload: any = {
        name: `${s.name} (Copia)`,
        season: s.season,
        status: s.status ?? 'active',
      }
      if (s.dateRange) {
        const parts = String(s.dateRange).split(' - ').map((p) => p.trim())
        if (parts[0]) payload.start_date = parts[0]
        if (parts[1]) payload.end_date = parts[1]
      }
      await createSeries(payload)
      await refreshDashboard()
      toast.success('Serie duplicada exitosamente!')
    } catch (e: any) {
      toast.error(e?.toString?.() ?? 'No se pudo duplicar la serie')
    }
  }

  const promptDeleteSeries = (seriesId: number) => {
    const s = series.find((x) => x.id === seriesId)
    if (!s) return
    setDeleteCandidate(s)
  }

  const handleCancelDelete = () => setDeleteCandidate(null)

  const handleConfirmDelete = async () => {
    if (!deleteCandidate) return
    const seriesId = deleteCandidate.id
    const s = deleteCandidate

    // optimistic remove from UI
    setSeries((prev) => prev.filter((x) => x.id !== seriesId))
    try {
      const idNum = Number(seriesId)
      if (!isNaN(idNum)) {
        await deleteSeries(idNum)
        // refresh to be safe
        await refreshDashboard()
      }
      toast.success(`Serie "${s.name}" eliminada`)
    } catch (e: any) {
      // on failure, try to re-sync state from server
      try {
        await refreshDashboard()
      } catch (_) {
        // ignore
      }
      toast.error(e?.toString?.() ?? 'No se pudo eliminar la serie')
    } finally {
      setDeleteCandidate(null)
    }
  }

  // search: use deferred value + normalization (case & accents insensitive)
  function norm(s?: string) {
    return (s ?? '')
      .toLowerCase()
      .normalize('NFD')
      .replace(/[\u0300-\u036f]/g, '')
  }

  const q = useDeferredValue(searchQuery)

  const filteredSeries = useMemo(() => {
    const needle = norm(q)
    if (!needle) return series
    return series.filter((s) => {
      const hay = [s.name, s.season, s.status, (s as any).description, (s as any).dateRange]
        .map(norm)
        .join(' ')
      return hay.includes(needle)
    })
  }, [series, q])

  // helper id removed (modal moved into EventsView)

  // Map backend SeriesRow -> frontend Series
  function mapSeriesRowToSeries(row: any): Series {
    const id = Number(row.id)
    const name = row.name ?? ''
    const season = row.season ?? ''
    const status = (row.status ?? 'active') as Series['status']
    const start = row.start_date || ''
    const end = row.end_date || ''
    const dateRange = start && end ? `${start} - ${end}` : start ? start : end ? end : ''

    return {
      id,
      name,
      season,
      status,
      dateRange,
      eventsCount: row.events_count ?? 0,
      progress: row.progress ?? 0,
      description: '',
    }
  }

  // Load real series from backend on mount; if it fails, keep initialSeries
  useEffect(() => {
    let mounted = true
    ;(async () => {
      try {
        const [seriesData, statsData, activityData] = await Promise.all([
            getSeries(),
            getDashboardStats(),
            getRecentActivity(20)
        ])
        if (!mounted) return
        setSeries((seriesData as any[]).map(mapSeriesRowToSeries))
        setDashboardStats(statsData)
        setRecentActivity(activityData)
      } catch (e) {
        // keep initialSeries if fetch fails
        // log for debugging
        // console.error('getSeries failed:', e)
      }
    })()
    return () => {
      mounted = false
    }
  }, [])

  // when rendering EventsView we force remount with key={selectedSeries.id}


  // Si hay serie y evento seleccionados, mostramos EventDetails.
  // Nota: EventCaptureView sólo se abre desde EventDetails cuando el usuario lo solicita.
  if (selectedEvent && selectedSeries) {
    return (
      <EventDetails
        event={selectedEvent}
        series={selectedSeries}
        onBack={handleBackToEvents}
      />
    )
  }

  // Vista de eventos si hay serie seleccionada
  if (selectedSeries) {
    return (
      <EventsView
        key={selectedSeries.id}
        series={selectedSeries}
        onBack={handleBackToSeries}
        onEditEvent={(ev) => handleViewEvent(ev)}
        
        onExportEvent={() => toast.success('Evento exportado')}
        // Por defecto estas acciones abren EventDetails (no la vista de captura).
        onGenerateDraw={(ev) => handleViewEvent(ev)}
        onRecordRuns={(ev) => handleViewEvent(ev)}
        onViewStandings={(ev) => handleViewEvent(ev)}
        onComputePayoffs={(ev) => handleViewEvent(ev)}
        onNavigate={onNavigate}
      />
    )
  }

  return (
    <div className="flex-1 h-full flex overflow-hidden bg-background">
      {/* Main Content */}
      <div className="flex-1 overflow-y-auto">
        <div className="p-6 lg:p-8">
          {/* Header */}
          <div className="flex justify-between items-start mb-8">
            <div>
              <h1 className="mb-2 text-foreground">Dashboard — Roping Manager</h1>
            </div>

            <div className="flex items-center gap-3">
              <div className="relative hidden md:block">
                <Search className="absolute left-3 top-1/2 -translate-y-1/2 text-muted-foreground size-4" />
                <Input
                  type="text"
                  placeholder="Buscar series..."
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  className="pl-10 w-64 bg-muted border-border rounded-xl h-11"
                />
              </div>

              {/* New Event removed from global header per UX: only available inside a series (EventsView) */}

              <Button onClick={() => setIsNewSeriesModalOpen(true)} className="bg-primary text-primary-foreground hover:opacity-90 rounded-xl shadow-sm">
                <Plus className="mr-2 size-4" />
                New Series
              </Button>
            </div>
          </div>

          {/* Series Grid */}
          <div className="mb-8">
            <h2 className="mb-6 text-foreground">Mis Series</h2>
            <div className="grid grid-cols-1 gap-6 md:grid-cols-2 lg:grid-cols-3">
              {filteredSeries.map((s) => (
                <SeriesCard
                  key={s.id}
                  series={s}
                  onView={() => handleViewSeries(s)}
                  onEdit={() => handleEditSeries(s.id)}
                  onDuplicate={() => handleDuplicateSeries(s.id)}
                  onDelete={() => promptDeleteSeries(s.id)}
                />
              ))}
            </div>

            {filteredSeries.length === 0 && (
              <div className="text-center py-12 bg-card rounded-xl border border-border">
                <p className="text-muted-foreground">No se encontraron series</p>
              </div>
            )}
          </div>

          {/* Actividad reciente */}
          <RecentActivity items={recentActivity} onViewAll={() => onNavigate?.('activity')} />
        </div>
      </div>

      {/* Panel de métricas */}
      <MetricsPanel stats={dashboardStats} />

      {/* Modales */}
      <NewSeriesModal
        isOpen={isNewSeriesModalOpen}
        onClose={() => {
          setIsNewSeriesModalOpen(false)
          setEditingSeries(null)
        }}
        initialValue={editingSeries}
          onCreateSeries={async (s: Series) => {
            // if editing, update local state first (optimistic) and try backend update
            if (editingSeries) {
              // replace by id (string) in state to avoid duplicates
              setSeries((prev) => prev.map((x) => (x.id === editingSeries.id ? { ...x, ...s } : x)))

              try {
                const idNum = Number(editingSeries.id)
                if (!isNaN(idNum)) {
                  const payload: any = {
                    name: s.name,
                    season: s.season,
                    status: s.status,
                  }
                  if (s.dateRange) {
                    const parts = String(s.dateRange).split(' - ').map((p) => p.trim())
                    if (parts[0]) payload.start_date = parts[0]
                    if (parts[1]) payload.end_date = parts[1]
                  }
                  await updateSeries(idNum, payload)
                  // refresh to get canonical data
                  await refreshDashboard()
                  toast.success(`Serie "${s.name}" actualizada`)
                } else {
                  // non-numeric id (local-only): we've already updated local state
                  toast.success(`Serie "${s.name}" actualizada (local)`)
                }
              } catch (e: any) {
                // update failed on backend: re-fetch to sync UI with server
                try {
                  await refreshDashboard()
                } catch (_) {
                  // ignore
                }
                toast.error(e?.toString?.() ?? 'No se pudo actualizar la serie en el backend')
              } finally {
                setEditingSeries(null)
                setIsNewSeriesModalOpen(false)
              }
            } else {
              await handleCreateSeries(s)
            }
          }}
      />

      {/* NewEventModal removed from Dashboard: controlled inside EventsView */}

      {/* Confirmación de eliminación de series */}
      <Dialog open={!!deleteCandidate} onOpenChange={(open) => { if (!open) handleCancelDelete() }}>
        <DialogContent className="sm:max-w-[520px]">
          <DialogHeader>
            <DialogTitle>Confirmar eliminación</DialogTitle>
            <DialogDescription>
              {deleteCandidate
                ? `¿Estás seguro de que quieres eliminar la serie "${deleteCandidate.name}"? Esta acción no se puede deshacer.`
                : 'Eliminar serie'}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <div className="flex items-center justify-end gap-2 w-full">
              <Button variant="outline" onClick={handleCancelDelete}>Cancelar</Button>
              <Button onClick={() => handleConfirmDelete()} className="bg-red-600 text-white">Eliminar</Button>
            </div>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
