import { useMemo, useState, useEffect } from 'react'
import { Button } from './ui/button'
import { Input } from './ui/input'
import { Switch } from './ui/switch'
import { Badge } from './ui/badge'
import {
  Table,
  TableHeader,
  TableBody,
  TableRow,
  TableHead,
  TableCell,
} from './ui/table'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from './ui/select'
import { getSeries, getEvents, exportEvent, exportEventBackup } from '../lib/api'
import { save } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'
import type { Series, Event } from '../types'
import { buildEventBackupFilename, getEventBackupErrorMessage } from '../lib/event-backup-ui'

type ExportSelectionOptions = Omit<Parameters<typeof exportEvent>[1], 'file_path'>
type SectionToggleState = Omit<ExportSelectionOptions, 'include_blocked'>

interface ExportHistoryRecord {
  id: string
  timestamp: string
  seriesName: string
  eventName: string
  eventId: number
  mode: 'full' | 'custom'
  options: ExportSelectionOptions
}

const HISTORY_STORAGE_KEY = 'rm-export-history-v1'

const DEFAULT_FULL_EXPORT: SectionToggleState = {
  overview: true,
  standings: true,
  run_order: true,
  teams: true,
  payoffs: true,
  event_logs: true,
}

const SECTION_LABELS: Record<keyof SectionToggleState, string> = {
  overview: 'Resumen',
  standings: 'Standings',
  run_order: 'Rondas',
  teams: 'Equipos',
  payoffs: 'Payoffs',
  event_logs: 'Logs',
}

const createHistoryId = () => {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return crypto.randomUUID()
  }
  return `export-${Date.now()}-${Math.random().toString(16).slice(2)}`
}

const sanitizeForFilename = (value: string) => value.replace(/[^a-z0-9]+/gi, '_').replace(/_+/g, '_').replace(/^_+|_+$/g, '')

export function ExportManagement() {
  const [seriesList, setSeriesList] = useState<Series[]>([])
  const [eventsList, setEventsList] = useState<Event[]>([])
  const [selectedSeriesId, setSelectedSeriesId] = useState<string | null>(null)
  const [selectedEventId, setSelectedEventId] = useState<string | null>(null)

  const [includeBlocked, setIncludeBlocked] = useState(false)
  const [sectionToggles, setSectionToggles] = useState<SectionToggleState>({
    overview: true,
    standings: true,
    run_order: true,
    teams: false,
    payoffs: true,
    event_logs: false,
  })
  const [query, setQuery] = useState('')
  const [exportHistory, setExportHistory] = useState<ExportHistoryRecord[]>([])

  // Load initial data
  useEffect(() => {
    getSeries().then((data) => {
      setSeriesList(data)
      const active = data.find((s) => s.status === 'active')
      if (active) setSelectedSeriesId(active.id.toString())
    })
  }, [])

  // Load events
  useEffect(() => {
    if (selectedSeriesId) {
      getEvents(parseInt(selectedSeriesId)).then((data) => {
        setEventsList(data)
        const active = data.find((e) => e.status === 'active' || e.status === 'completed')
        if (active) setSelectedEventId(active.id.toString())
        else setSelectedEventId(null)
      })
    } else {
      setEventsList([])
    }
  }, [selectedSeriesId])

  useEffect(() => {
    if (typeof window === 'undefined') return
    try {
      const stored = window.localStorage.getItem(HISTORY_STORAGE_KEY)
      if (stored) {
        const parsed = JSON.parse(stored) as ExportHistoryRecord[]
        setExportHistory(parsed)
      }
    } catch (error) {
      console.warn('No fue posible leer el historial de exportaciones', error)
    }
  }, [])

  useEffect(() => {
    if (typeof window === 'undefined') return
    try {
      window.localStorage.setItem(HISTORY_STORAGE_KEY, JSON.stringify(exportHistory))
    } catch (error) {
      console.warn('No fue posible guardar el historial de exportaciones', error)
    }
  }, [exportHistory])

  const filteredHistory = useMemo(() => {
    const q = query.trim().toLowerCase()
    return [...exportHistory]
      .sort((a, b) => (a.timestamp < b.timestamp ? 1 : -1))
      .filter((record) => {
        if (!q) return true
        return `${record.seriesName} ${record.eventName}`.toLowerCase().includes(q)
      })
  }, [exportHistory, query])

  function toggleType(k: keyof SectionToggleState) {
    setSectionToggles((prev) => ({ ...prev, [k]: !prev[k] }))
  }

  const appendHistoryRecord = (entry: Omit<ExportHistoryRecord, 'id' | 'timestamp'>) => {
    const snapshot: ExportHistoryRecord = {
      ...entry,
      id: createHistoryId(),
      timestamp: new Date().toISOString(),
      options: { ...entry.options },
    }
    setExportHistory((prev) => [snapshot, ...prev].slice(0, 25))
  }

  const buildOptionsPayload = (full: boolean): ExportSelectionOptions => {
    const base = full ? DEFAULT_FULL_EXPORT : sectionToggles
    return {
      ...base,
      include_blocked: includeBlocked,
    }
  }

  const describeRecordType = (record: ExportHistoryRecord) => {
    const enabled = Object.entries(SECTION_LABELS)
      .filter(([key]) => record.options[key as keyof SectionToggleState])
      .map(([, label]) => label)
    const isFull =
      record.mode === 'full' || enabled.length === Object.keys(SECTION_LABELS).length
    const baseLabel = isFull ? 'XLSX Completo' : `Selección (${enabled.join(', ')})`
    return record.options.include_blocked ? `${baseLabel} + bloqueados` : baseLabel
  }

  const formatTimestamp = (iso: string) => {
    const date = new Date(iso)
    if (Number.isNaN(date.getTime())) return iso
    return date.toLocaleString()
  }

  const executeExport = async (
    eventId: number,
    optionsPayload: ExportSelectionOptions,
    metadata: { mode: 'full' | 'custom'; seriesName: string; eventName: string }
  ) => {
    const safeSeries = sanitizeForFilename(metadata.seriesName || 'Serie')
    const safeEvent = sanitizeForFilename(metadata.eventName || 'Evento')
    const defaultPath = `${safeSeries || 'Serie'}_${safeEvent || 'Evento'}.xlsx`
    const filePath = await save({
      filters: [{ name: 'Excel', extensions: ['xlsx'] }],
      defaultPath,
    })
    if (!filePath) return

    try {
      await exportEvent(eventId, { ...optionsPayload, file_path: filePath })
      toast.success('Exportación completada exitosamente')
      appendHistoryRecord({
        eventId,
        options: { ...optionsPayload },
        mode: metadata.mode,
        seriesName: metadata.seriesName,
        eventName: metadata.eventName,
      })
    } catch (error) {
      console.error(error)
      toast.error('Error al exportar el archivo')
    }
  }

  const handleExport = async (full: boolean = false) => {
    if (!selectedEventId) {
      toast.error('Selecciona un evento primero')
      return
    }

    if (!full && !Object.values(sectionToggles).some(Boolean)) {
      toast.error('Selecciona al menos un módulo para exportar')
      return
    }

    const eventId = parseInt(selectedEventId, 10)
    const eventData = eventsList.find((e) => e.id === eventId)
    const seriesData = seriesList.find((s) => s.id.toString() === selectedSeriesId)
    const optionsPayload = buildOptionsPayload(full)

    await executeExport(eventId, optionsPayload, {
      mode: full ? 'full' : 'custom',
      seriesName: seriesData?.name ?? 'Serie',
      eventName: eventData?.name ?? `Evento ${selectedEventId}`,
    })
  }

  const handleHistoryReexport = async (record: ExportHistoryRecord) => {
    await executeExport(record.eventId, { ...record.options }, {
      mode: record.mode,
      seriesName: record.seriesName,
      eventName: record.eventName,
    })
  }

  const handleExportBackup = async () => {
    if (!selectedEventId) {
      toast.error('Selecciona un evento primero')
      return
    }

    const eventId = parseInt(selectedEventId, 10)
    const eventData = eventsList.find((e) => e.id === eventId)
    const seriesData = seriesList.find((s) => s.id.toString() === selectedSeriesId)
    const defaultPath = buildEventBackupFilename(
      seriesData?.name ?? 'Serie',
      eventData?.name ?? `Evento_${selectedEventId}`
    )

    const filePath = await save({
      filters: [{ name: 'Backup de evento', extensions: ['xlsx'] }],
      defaultPath,
    })
    if (!filePath) return

    const toastId = toast.loading('Generando backup del evento...')

    try {
      await exportEventBackup(eventId, filePath)
      toast.dismiss(toastId)
      toast.success('Backup exportado', {
        description: 'Se generó un archivo de restauración separado del reporte de resultados.',
      })
    } catch (error) {
      toast.dismiss(toastId)
      console.error(error)
      toast.error('No se pudo exportar el backup', {
        description: getEventBackupErrorMessage(error, 'Revisa el archivo o intenta de nuevo.'),
      })
    }
  }

  const handleRefresh = async () => {
    try {
      const series = await getSeries()
      setSeriesList(series)
      if (selectedSeriesId) {
        const refreshedEvents = await getEvents(parseInt(selectedSeriesId, 10))
        setEventsList(refreshedEvents)
      } else {
        setEventsList([])
      }
    } catch (error) {
      console.error(error)
      toast.error('No se pudo refrescar la información')
    }
  }

  return (
    <div className="p-6 h-full flex flex-col overflow-hidden">
      <div className="flex-1 overflow-y-auto">
        <div className="mb-6 flex items-start justify-between">
          <div>
            <h1 className="text-2xl font-semibold text-foreground">Exportar Resultados de Eventos</h1>
            <p className="text-sm text-muted-foreground">Descarga reportes detallados en formato Excel (XLSX)</p>
          </div>

          <div className="flex items-center gap-3">
            <Button variant="ghost" onClick={handleRefresh}>Refrescar</Button>
          </div>
        </div>

        <div className="bg-card border border-border rounded-xl p-6 mb-6">
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
            <Select value={selectedSeriesId || ''} onValueChange={setSelectedSeriesId}>
              <SelectTrigger className="w-[180px] rounded-lg border border-border bg-card px-3 py-2 text-sm">
                <SelectValue placeholder="Serie" />
              </SelectTrigger>
              <SelectContent>
                {seriesList.map(s => <SelectItem key={s.id} value={s.id.toString()}>{s.name}</SelectItem>)}
              </SelectContent>
            </Select>

            <Select value={selectedEventId || ''} onValueChange={setSelectedEventId} disabled={!selectedSeriesId}>
              <SelectTrigger className="w-[180px] rounded-lg border border-border bg-card px-3 py-2 text-sm">
                <SelectValue placeholder="Evento" />
              </SelectTrigger>
              <SelectContent>
                {eventsList.map(e => <SelectItem key={e.id} value={e.id.toString()}>{e.name}</SelectItem>)}
              </SelectContent>
            </Select>
          </div>

          <div className="mt-6">
            <div className="text-sm font-medium mb-2">Tipo de Exportación</div>
            <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
              <button type="button" onClick={() => toggleType('standings')} className={`text-left rounded-md p-4 border ${sectionToggles.standings ? 'border-orange-200 bg-orange-50' : 'border-border bg-card'}`}>
                <div className="font-medium">Resultados globales</div>
                <div className="text-xs text-muted-foreground">Standings completos del evento</div>
              </button>

              <button type="button" onClick={() => toggleType('run_order')} className={`text-left rounded-md p-4 border ${sectionToggles.run_order ? 'border-orange-200 bg-orange-50' : 'border-border bg-card'}`}>
                <div className="font-medium">Resultados por ronda</div>
                <div className="text-xs text-muted-foreground">Detalle de cada ronda</div>
              </button>

              <button type="button" onClick={() => toggleType('teams')} className={`text-left rounded-md p-4 border ${sectionToggles.teams ? 'border-orange-200 bg-orange-50' : 'border-border bg-card'}`}>
                <div className="font-medium">Equipos</div>
                <div className="text-xs text-muted-foreground">Lista de equipos participantes</div>
              </button>

              <button type="button" onClick={() => toggleType('payoffs')} className={`text-left rounded-md p-4 border ${sectionToggles.payoffs ? 'border-orange-200 bg-orange-50' : 'border-border bg-card'}`}>
                <div className="font-medium">Payoffs</div>
                <div className="text-xs text-muted-foreground">Distribución de premios</div>
              </button>

              <button type="button" onClick={() => toggleType('event_logs')} className={`text-left rounded-md p-4 border ${sectionToggles.event_logs ? 'border-orange-200 bg-orange-50' : 'border-border bg-card'}`}>
                <div className="font-medium">Logs / Exclusiones</div>
                <div className="text-xs text-muted-foreground">Registro de cambios y exclusiones</div>
              </button>
            </div>
          </div>

          <div className="mt-6 border-t border-border pt-4">
            <div className="flex items-center justify-between">
              <div className="text-sm text-muted-foreground">Incluir datos bloqueados</div>
              <Switch checked={includeBlocked} onCheckedChange={(v) => setIncludeBlocked(Boolean(v))} />
            </div>
          </div>
        </div>

        <div className="flex flex-col items-center gap-3 mb-6">
          <Button className="bg-orange-500 hover:bg-orange-600 text-white w-64" onClick={() => handleExport(true)} disabled={!selectedEventId}>
            ⬇ Exportar XLSX Completo
          </Button>
          <Button variant="outline" className="w-48" onClick={() => handleExport(false)} disabled={!selectedEventId}>
            Exportar Selección
          </Button>
          <Button variant="secondary" className="w-48" onClick={handleExportBackup} disabled={!selectedEventId}>
            Exportar Backup
          </Button>
        </div>

        <div className="bg-card border border-border rounded-xl p-4">
          <h3 className="font-medium mb-2">Historial de Exportaciones Recientes</h3>
          <div className="mt-2">
            <Input placeholder="Buscar en historial..." value={query} onChange={(e) => setQuery(e.target.value)} />
          </div>

          <div className="mt-4">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Fecha</TableHead>
                  <TableHead>Serie</TableHead>
                  <TableHead>Evento</TableHead>
                  <TableHead>Tipo</TableHead>
                  <TableHead>Tamaño</TableHead>
                  <TableHead>Estado</TableHead>
                  <TableHead>Acciones</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {filteredHistory.length === 0 ? (
                  <TableRow>
                    <TableCell colSpan={7} className="text-center text-sm text-muted-foreground">
                      No hay exportaciones registradas.
                    </TableCell>
                  </TableRow>
                ) : (
                  filteredHistory.map((record) => (
                    <TableRow key={record.id}>
                      <TableCell className="text-sm text-muted-foreground">{formatTimestamp(record.timestamp)}</TableCell>
                      <TableCell>{record.seriesName}</TableCell>
                      <TableCell>{record.eventName}</TableCell>
                      <TableCell>{describeRecordType(record)}</TableCell>
                      <TableCell className="text-sm text-muted-foreground">—</TableCell>
                      <TableCell>
                        <Badge className="bg-emerald-50 text-emerald-700 border-emerald-200">Completado</Badge>
                      </TableCell>
                      <TableCell className="text-right">
                        <Button variant="link" className="px-0 text-orange-600" onClick={() => handleHistoryReexport(record)}>
                          Re-exportar
                        </Button>
                      </TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
          </div>
        </div>
      </div>
    </div>
  )
}
