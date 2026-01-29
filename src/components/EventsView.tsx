import { useState, useMemo, useEffect } from 'react'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from './ui/dialog'
import { Search, Plus, Grid, List, ArrowLeft } from 'lucide-react'
import { Button } from './ui/button'
import { Input } from './ui/input'
import { Tabs, TabsList, TabsTrigger } from './ui/tabs'
import { SeriesOverviewCard } from './SeriesOverviewCard'
import { EventCard } from './EventCard'
import { InsightsPanel } from './InsightsPanel'
import { NewEventModal } from './NewEventModal'
import { toast } from 'sonner'
import { getEvents, createEvent, duplicateEvent, deleteEvent, updateEvent } from '@/lib/api'
import type { Series, Event } from '../types'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

interface EventsViewProps {
  series: Series
  onBack: () => void
  onEditEvent: (e: Event) => void
  // onDuplicateEvent removed: duplication handled internally
  onExportEvent: (e: Event) => void
  // Acciones opcionales: no deben forzar la navegación a captura por defecto.
  onGenerateDraw?: (e: Event) => void
  onRecordRuns?: (e: Event) => void
  onViewStandings?: (e: Event) => void
  onComputePayoffs?: (e: Event) => void
  onNavigate?: (item: string) => void
}

export function EventsView({
  series,
  onBack,
  onEditEvent,
  
  onExportEvent,
  onGenerateDraw,
  onRecordRuns,
  onViewStandings,
  onComputePayoffs,
  onNavigate,
}: EventsViewProps) {
  const [query, setQuery] = useState('')
  const [view, setView] = useState<'cards' | 'table'>('cards')
  const [status, setStatus] = useState<'all' | 'active' | 'locked' | 'draft'>('all')
  const [isNewEventOpen, setIsNewEventOpen] = useState(false)
  const [events, setEvents] = useState<Event[]>([])
  const [loading, setLoading] = useState(true)
  const [deleteCandidateEvent, setDeleteCandidateEvent] = useState<Event | null>(null)
  const [deletePin, setDeletePin] = useState('')
  const [editCandidateEvent, setEditCandidateEvent] = useState<Event | null>(null)
  const [editPin, setEditPin] = useState('')
  const [editEvent, setEditEvent] = useState<Event | null>(null)

  function mapEventRow(row: any) {
    function normStatus(s: any) {
      const v = String(s ?? 'upcoming').toLowerCase()
      if (v === 'finalized') return 'completed'
      if (v === 'upcoming') return 'draft'
      return v
    }
    return {
      id: Number(row.id),
      seriesId: Number(row.series_id ?? row.seriesId ?? 0),
      name: row.name ?? '',
      date: String((row.date ?? row.event_date ?? '')).slice(0,10),
      status: normStatus(row.status),
      rounds: Number(row.rounds ?? 1),
      teamsCount: Number(row.teams_count ?? 0),
      entryFee: row.entry_fee ?? undefined,
      maxTeamRating: row.max_team_rating ?? undefined,
      pot: Number(row.pot ?? 0),
      payoffAllocation: row.payoff_allocation ?? undefined,
      location: row.location ?? undefined,
      prizePool: row.prize_pool ?? undefined,
      adminPin: row.admin_pin ?? undefined,
    } as Event
  }

  async function load() {
    setLoading(true)
    try {
      const sid = Number(series.id)
      const data = await getEvents(Number.isFinite(sid) ? sid : undefined)
      // Map rows and defensively filter by series id (string) to avoid showing
      // events from other series in case the backend returned more than expected.
      const sidStr = String(series.id ?? '')
      const mapped = (data as any[]).map(mapEventRow)
      const filteredBySeries = mapped.filter((ev) => String(ev.seriesId) === sidStr)
      try {
        // debug help: show what the backend returned vs what we filtered
        // eslint-disable-next-line no-console
        console.debug('[EventsView] load', { seriesId: sidStr, returned: mapped.length, afterFilter: filteredBySeries.length })
      } catch (e) {}
      setEvents(filteredBySeries)
    } catch (e) {
      toast.error('No se pudieron cargar los eventos')
    } finally {
      setLoading(false)
    }
  }

  const handleDuplicate = async (ev: Event) => {
    try {
      const idNum = Number(ev.id)
      if (isNaN(idNum)) throw new Error('ID inválido')
        await duplicateEvent(idNum)
        toast.success('Evento duplicado')
      await load()
    } catch (err: any) {
      toast.error(err?.toString?.() ?? 'No se pudo duplicar el evento')
    }
  }

  const handleEdit = (ev: Event) => {
    if (ev.adminPin) {
      setEditCandidateEvent(ev)
      setEditPin('')
    } else {
      setEditEvent(ev)
    }
  }

  const cancelEditCandidate = () => {
    setEditCandidateEvent(null)
    setEditPin('')
  }

  const confirmEditCandidate = () => {
    if (!editCandidateEvent) return
    if (editCandidateEvent.adminPin && editCandidateEvent.adminPin !== editPin) {
      toast.error('PIN incorrecto')
      return
    }
    setEditEvent(editCandidateEvent)
    setEditCandidateEvent(null)
    setEditPin('')
  }

  const handleDelete = async (ev: Event) => {
    // replaced by dialog-based confirmation: set candidate and open dialog
    setDeleteCandidateEvent(ev)
    setDeletePin('')
  }

  const cancelDeleteEvent = () => {
    setDeleteCandidateEvent(null)
    setDeletePin('')
  }

  const confirmDeleteEvent = async () => {
    if (!deleteCandidateEvent) return

    if (deleteCandidateEvent.adminPin && deleteCandidateEvent.adminPin !== deletePin) {
      toast.error('PIN incorrecto')
      return
    }

    try {
      const idNum = Number(deleteCandidateEvent.id)
      if (isNaN(idNum)) throw new Error('ID inválido')
      await deleteEvent(idNum)
      toast.success('Evento eliminado')
      await load()
    } catch (err: any) {
      toast.error(err?.toString?.() ?? 'No se pudo eliminar el evento')
    } finally {
      setDeleteCandidateEvent(null)
      setDeletePin('')
    }
  }

  useEffect(() => {
    load()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [series.id])

  useEffect(() => {
    // close modal when series changes to ensure a clean form
    setIsNewEventOpen(false)
  }, [series.id])

  const filtered = useMemo(() => {
    return events
      .filter((e) =>
        status === 'all' ? true : e.status === status
      )
      .filter((e) =>
        e.name.toLowerCase().includes(query.toLowerCase())
      )
  }, [events, query, status])

  return (
    <div className="flex h-full bg-background">
      {/* Main */}
      <div className="flex-1 overflow-y-auto">
        <div className="p-6 lg:p-8">
          {/* Header */}
          <div className="flex items-center justify-between mb-6">
            <div className="flex items-center gap-3">
              <Button variant="ghost" size="icon" onClick={onBack} className="rounded-xl hover:bg-accent">
                <ArrowLeft className="size-5" />
              </Button>
              <div>
                <h1 className="text-foreground">{series.name}</h1>
                <p className="text-sm text-muted-foreground">
                  {series.season} • {series.eventsCount} eventos
                </p>
              </div>
            </div>

            <div className="flex items-center gap-2">
              <div className="hidden md:block relative">
                <Search className="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
                <Input
                  placeholder="Buscar evento..."
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  className="pl-10 w-64 bg-muted border-border rounded-xl h-11"
                />
              </div>
              <Button
                onClick={() => setIsNewEventOpen(true)}
                className="bg-primary text-primary-foreground rounded-xl shadow-sm hover:opacity-90"
              >
                <Plus className="mr-2 size-4" />
                Nuevo evento
              </Button>
            </div>
          </div>

          {/* Overview */}
          <SeriesOverviewCard series={series} events={events} />

          {/* Filtros & vista */}
          <div className="flex items-center justify-between mb-4">
            <Tabs
              value={status}
              onValueChange={(v: any) => setStatus(v)}
              className="w-auto"
            >
              <TabsList>
                <TabsTrigger value="all">Todos</TabsTrigger>
                <TabsTrigger value="active">Activos</TabsTrigger>
                <TabsTrigger value="locked">Bloqueados</TabsTrigger>
                <TabsTrigger value="draft">Draft</TabsTrigger>
              </TabsList>
            </Tabs>

            <div className="flex items-center gap-2">
              <Button
                variant={view === 'cards' ? 'default' : 'outline'}
                size="icon"
                onClick={() => setView('cards')}
                className={view === 'cards' ? 'bg-primary text-primary-foreground' : 'border-border'}
                aria-label="Vista de tarjetas"
              >
                <Grid className="size-4" />
              </Button>
              <Button
                variant={view === 'table' ? 'default' : 'outline'}
                size="icon"
                onClick={() => setView('table')}
                className={view === 'table' ? 'bg-primary text-primary-foreground' : 'border-border'}
                aria-label="Vista de lista"
              >
                <List className="size-4" />
              </Button>
            </div>
          </div>

          {loading && <div className="mb-4 text-sm text-muted-foreground">Cargando eventos…</div>}

          {/* Listado */}
          {view === 'cards' ? (
            <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-6">
              {filtered.map((event) => (
                <EventCard
                  key={event.id}
                  event={event}
                  onViewTeams={() => onEditEvent(event)}
                  onEdit={() => handleEdit(event)}
                  onGenerateDraw={() => onGenerateDraw?.(event)}
                  onRecordRuns={() => onRecordRuns?.(event)}
                  onViewStandings={() => (onViewStandings ? onViewStandings(event) : onNavigate?.('resultados'))}
                  onComputePayoffs={() => onComputePayoffs?.(event)}
                  onExport={() => onExportEvent(event)}
                  onDuplicate={() => handleDuplicate(event)}
                  onDelete={() => handleDelete(event)}
                />
              ))}

              {filtered.length === 0 && (
                <div className="col-span-full text-center py-12 bg-card border border-border rounded-xl">
                  <p className="text-muted-foreground">No hay eventos con ese filtro</p>
                </div>
              )}
            </div>
          ) : (
            <div className="bg-card border border-border rounded-xl p-6">
              {/* Aquí puedes colocar tu versión en tabla si la tienes */}
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead className="text-muted-foreground">Nombre</TableHead>
                    <TableHead className="text-muted-foreground">Fecha</TableHead>
                    <TableHead className="text-muted-foreground">Estado</TableHead>
                    <TableHead className="text-muted-foreground">Rondas</TableHead>
                    <TableHead className="text-muted-foreground">Equipos</TableHead>
                    <TableHead className="text-muted-foreground">Acciones</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {filtered.length === 0 ? (
                    <TableRow>
                      <TableCell colSpan={6} className="text-center py-4 text-muted-foreground">
                        No hay eventos con ese filtro
                      </TableCell>
                    </TableRow>
                  ) : (
                    filtered.map((event) => (
                      <TableRow key={event.id}>
                        <TableCell className="font-medium">{event.name}</TableCell>
                        <TableCell>{event.date}</TableCell>
                        <TableCell>{event.status}</TableCell>
                        <TableCell>{event.rounds}</TableCell>
                        <TableCell>{event.teamsCount}</TableCell>
                        <TableCell>
                          <div className="flex justify-center gap-2">
                            <Button variant="outline" onClick={() => handleEdit(event)}>
                              Editar
                            </Button>
                            <Button variant="outline" onClick={() => handleDuplicate(event)}>
                              Duplicar
                            </Button>
                            <Button variant="destructive" onClick={() => handleDelete(event)}>
                              Eliminar
                            </Button>
                          </div>
                        </TableCell>
                      </TableRow>
                    ))
                  )}
                </TableBody>
              </Table>
            </div>
          )}
        </div>
      </div>

      {/* Insights */}
  <InsightsPanel events={events} seriesId={Number(series.id)} />

      {/* New Event Modal controlled locally in EventsView */}
      <NewEventModal
        isOpen={isNewEventOpen}
        onClose={() => setIsNewEventOpen(false)}
        onCreateEvent={async (e: Event) => {
          // create on backend then reload
          try {
            const sid = Number(e.seriesId ?? series.id)
            if (!Number.isFinite(sid)) throw new Error('series id inválido al crear evento')
            await createEvent({
              series_id: sid,
              name: e.name,
              date: e.date,
              rounds: e.rounds || 1,
              status: e.status as any ?? 'draft',
              location: e.location ?? null,
              entry_fee: e.entryFee ?? null,
              prize_pool: e.prizePool ?? null,
              max_team_rating: e.maxTeamRating ?? null,
              payoff_allocation: e.payoffAllocation ?? null,
              admin_pin: e.adminPin ?? null,
            })
            toast.success('Evento creado')
            await load()
          } catch (err) {
            toast.error('No se pudo crear el evento')
          } finally {
            setIsNewEventOpen(false)
          }
        }}
        seriesId={series.id}
      />

      {/* Edit Event Modal */}
      <NewEventModal
        isOpen={!!editEvent}
        onClose={() => setEditEvent(null)}
        initialEvent={editEvent ?? undefined}
        onUpdateEvent={async (id: string, patch: any) => {
          try {
            await updateEvent(Number(id), patch)
            toast.success('Evento actualizado')
            await load()
          } catch (err: any) {
            toast.error(err?.toString?.() ?? 'No se pudo actualizar el evento')
          } finally {
            setEditEvent(null)
          }
        }}
        seriesId={series.id}
      />

      {/* Confirmación PIN para editar */}
      <Dialog open={!!editCandidateEvent} onOpenChange={(open: boolean) => { if (!open) cancelEditCandidate() }}>
        <DialogContent className="sm:max-w-[400px]">
          <DialogHeader>
            <DialogTitle>Ingresar PIN</DialogTitle>
            <DialogDescription>
              Este evento está protegido. Ingresa el PIN para editarlo.
            </DialogDescription>
          </DialogHeader>

          <div className="py-4">
            <Input
              value={editPin}
              onChange={(e) => setEditPin(e.target.value)}
              type="password"
              placeholder="PIN del evento"
              autoFocus
              onKeyDown={(e) => {
                if (e.key === 'Enter') confirmEditCandidate()
              }}
            />
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={cancelEditCandidate}>Cancelar</Button>
            <Button onClick={confirmEditCandidate}>Confirmar</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Confirmación de eliminación de evento */}
  <Dialog open={!!deleteCandidateEvent} onOpenChange={(open: boolean) => { if (!open) cancelDeleteEvent() }}>
        <DialogContent className="sm:max-w-[520px]">
          <DialogHeader>
            <DialogTitle>Confirmar eliminación</DialogTitle>
            <DialogDescription>
              {deleteCandidateEvent
                ? `¿Estás seguro de que quieres eliminar el evento "${deleteCandidateEvent.name}"? Esta acción no se puede deshacer.`
                : 'Eliminar evento'}
            </DialogDescription>
          </DialogHeader>

          {deleteCandidateEvent?.adminPin && (
            <div className="py-4">
              <label className="text-sm font-medium mb-2 block">Ingresa el PIN del evento para confirmar:</label>
              <Input
                value={deletePin}
                onChange={(e) => setDeletePin(e.target.value)}
                type="password"
                placeholder="PIN del evento"
                autoFocus
              />
            </div>
          )}

          <DialogFooter>
            <div className="flex items-center justify-end gap-2 w-full">
              <Button variant="outline" onClick={cancelDeleteEvent}>Cancelar</Button>
              <Button 
                onClick={() => confirmDeleteEvent()} 
                className="bg-red-600 text-white"
                disabled={!!deleteCandidateEvent?.adminPin && !deletePin}
              >
                Eliminar
              </Button>
            </div>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}

// ensure the modal closes when the series changes
// we implement this effect inside the component
// (placed here to avoid linter warnings about unused imports)
// Note: effect declared above using useEffect hook
