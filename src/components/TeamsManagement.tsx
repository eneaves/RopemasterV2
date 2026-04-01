import { useState, useMemo, useEffect } from 'react'
import { Button } from './ui/button'
import { Input } from './ui/input'
import {
  Table,
  TableHeader,
  TableBody,
  TableRow,
  TableHead,
  TableCell,
} from './ui/table'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './ui/select'
import { useTeams } from '@/hooks/useTeams'
import { useRopers } from '@/hooks/useRopers'
import { RefreshCw } from 'lucide-react'
import { Badge } from './ui/badge'
import { Alert, AlertDescription, AlertTitle } from './ui/alert'
import { useSeriesEvents } from '@/providers/SeriesEventsProvider'

export function TeamsManagement() {
  const { series: seriesList, events: eventsCatalog, loading: catalogLoading, refreshEvents, refreshSeries } = useSeriesEvents()
  const [selectedSeriesId, setSelectedSeriesId] = useState<string>('all')
  const [selectedEventId, setSelectedEventId] = useState<string>('')

  // Filter events based on selected series
  const filteredEvents = useMemo(() => {
    if (selectedSeriesId === 'all') return eventsCatalog
    return eventsCatalog.filter((event) => String(event.series_id ?? event.seriesId) === selectedSeriesId)
  }, [eventsCatalog, selectedSeriesId])

  // Update selected event if it becomes invalid due to series filter
  useEffect(() => {
    if (!selectedEventId && filteredEvents.length > 0) {
      setSelectedEventId(String(filteredEvents[0].id))
      return
    }

    if (selectedEventId && filteredEvents.length > 0) {
      const exists = filteredEvents.some((event) => String(event.id) === selectedEventId)
      if (!exists) {
        setSelectedEventId(String(filteredEvents[0].id))
      }
    }

    if (filteredEvents.length === 0 && selectedEventId) {
      setSelectedEventId('')
    }
  }, [filteredEvents, selectedEventId])

  const eventIdNum = Number(selectedEventId) || 0
  const { teams, loading, err, refresh } = useTeams(eventIdNum, false)
  const { ropers } = useRopers()

  const selectedEvent = filteredEvents.find(e => String(e.id) === selectedEventId)
  const maxRating = selectedEvent?.max_team_rating ?? 0

  const enrichedTeams = useMemo(() => {
    return teams.map(t => {
      const header = ropers.find(r => r.id === t.header_id)
      const heeler = ropers.find(r => r.id === t.heeler_id)
      return {
        ...t,
        headerName: header ? `${header.firstName} ${header.lastName}` : `ID: ${t.header_id}`,
        heelerName: heeler ? `${heeler.firstName} ${heeler.lastName}` : `ID: ${t.heeler_id}`,
        headerRating: header?.rating ?? '?',
        heelerRating: heeler?.rating ?? '?',
        // status is already in 't' but we might want to normalize it
      }
    })
  }, [teams, ropers])


  return (
    <div className="p-6 h-full">
      <div className="max-w-full">
        <div className="mb-6 flex items-start justify-between">
          <div>
            <h1 className="text-2xl font-semibold text-foreground">Gestión de Equipos</h1>
            <p className="text-sm text-muted-foreground">Vista global para administrar equipos por evento</p>
          </div>

          <div className="flex items-center gap-3">
            <Button
              variant="ghost"
              size="icon"
              onClick={() => {
                refresh()
                refreshEvents()
                refreshSeries()
              }}
              title="Recargar"
            >
              <RefreshCw className={`size-4 ${(loading || catalogLoading) ? 'animate-spin' : ''}`} />
            </Button>
          </div>
        </div>

        {err && (
            <Alert variant="destructive" className="mb-4">
                <AlertTitle>Error</AlertTitle>
                <AlertDescription>{err}</AlertDescription>
            </Alert>
        )}

        {/* Filters */}
        <div className="grid grid-cols-1 gap-4 md:grid-cols-3 mb-4">
          <Select value={selectedSeriesId} onValueChange={setSelectedSeriesId}>
            <SelectTrigger className="bg-card border-border">
                <SelectValue placeholder="Seleccionar Serie" />
            </SelectTrigger>
            <SelectContent side="bottom" sideOffset={8}>
                <SelectItem value="all">Todas las series</SelectItem>
                {seriesList.map(s => (
                    <SelectItem key={s.id} value={String(s.id)}>{s.name}</SelectItem>
                ))}
            </SelectContent>
          </Select>

          <Select value={selectedEventId} onValueChange={setSelectedEventId}>
            <SelectTrigger className="bg-card border-border">
                <SelectValue placeholder="Seleccionar Evento" />
            </SelectTrigger>
            <SelectContent side="bottom" sideOffset={8}>
                {filteredEvents.map(e => (
                    <SelectItem key={e.id} value={String(e.id)}>{e.name}</SelectItem>
                ))}
            </SelectContent>
          </Select>

          <div className="flex items-center gap-2">
            <Input placeholder="Buscar por nombre de roper..." />
          </div>
        </div>

        {/* Debug Info (Temporary) */}
        <div className="text-xs text-muted-foreground mb-2">
            Debug: EventID: {selectedEventId} | Teams: {teams.length} | Loading: {String(loading || catalogLoading)}
        </div>

        {/* Summary panel */}
        <div className="bg-card border border-border rounded-xl p-4 mb-6">
          <div className="grid grid-cols-2 gap-4 md:grid-cols-6 items-center">
            <div className="col-span-2 md:col-span-1 text-sm text-muted-foreground">Evento<br/><span className="text-foreground">{selectedEvent?.name || '-'}</span></div>
            <div className="col-span-2 md:col-span-1 text-sm text-muted-foreground">Max Rating<br/><span className="text-foreground">{maxRating > 0 ? maxRating : '-'}</span></div>
            <div className="col-span-2 md:col-span-1 text-center">
              <div className="text-sm text-muted-foreground">Total Equipos</div>
              <div className="text-2xl font-semibold">{teams.length}</div>
            </div>
            <div className="col-span-2 md:col-span-1 text-center">
              <div className="text-sm text-muted-foreground">Ropers Disp.</div>
              <div className="text-2xl font-semibold">{ropers.length}</div>
            </div>
            {/* ... */}
          </div>
        </div>

        {/* Table */}
        <div className="bg-card border border-border rounded-xl p-4">
          <div className="max-h-[60vh] overflow-auto">
            <Table className="min-w-full">
              <TableHeader>
                <TableRow>
                  <TableHead>ID</TableHead>
                  <TableHead>Header</TableHead>
                  <TableHead>Heeler</TableHead>
                  <TableHead>Rating Header</TableHead>
                  <TableHead>Rating Heeler</TableHead>
                  <TableHead>Team Rating</TableHead>
                  <TableHead>Estado</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {loading ? (
                   <TableRow><TableCell colSpan={7} className="text-center">Cargando...</TableCell></TableRow>
                ) : !selectedEventId ? (
                   <TableRow><TableCell colSpan={7} className="text-center">Selecciona un evento para ver los equipos</TableCell></TableRow>
                ) : enrichedTeams.length === 0 ? (
                   <TableRow><TableCell colSpan={7} className="text-center">No hay equipos registrados</TableCell></TableRow>
                ) : (
                  enrichedTeams.map((t) => (
                  <TableRow key={t.id}>
                    <TableCell className="font-medium">{t.id}</TableCell>
                    <TableCell>{t.headerName}</TableCell>
                    <TableCell>{t.heelerName}</TableCell>
                    <TableCell><span className="inline-flex items-center justify-center rounded-full bg-muted/20 px-2 py-1 text-xs">{t.headerRating}</span></TableCell>
                    <TableCell><span className="inline-flex items-center justify-center rounded-full bg-muted/20 px-2 py-1 text-xs">{t.heelerRating}</span></TableCell>
                    <TableCell><span className="inline-flex items-center justify-center rounded-full bg-muted/20 px-2 py-1 text-sm">{t.rating}</span></TableCell>
                    <TableCell>
                      <Badge variant={t.status === 'active' || t.status === 'valid' ? 'default' : 'destructive'}>
                          {t.status}
                      </Badge>
                    </TableCell>
                  </TableRow>
                )))}
              </TableBody>
            </Table>
          </div>
        </div>
      </div>

    </div>
  )
}
