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
import { getSeries, getEvents, exportEvent } from '../lib/api'
import { save } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'
import type { Series, Event } from '../types'

// Mock history for now as backend doesn't persist export history specifically
const mockExports = [
  { id: 'e1', date: '2025-02-04', series: 'Winter Classic', event: '#9 Roping', type: 'Full XLSX', size: '1.2 MB', status: 'Completado' },
]

export function ExportManagement() {
  const [seriesList, setSeriesList] = useState<Series[]>([])
  const [eventsList, setEventsList] = useState<Event[]>([])
  const [selectedSeriesId, setSelectedSeriesId] = useState<string | null>(null)
  const [selectedEventId, setSelectedEventId] = useState<string | null>(null)

  const [includeBlocked, setIncludeBlocked] = useState(false)
  const [types, setTypes] = useState({ 
    overview: true, 
    standings: true, 
    run_order: true, 
    teams: false, 
    payoffs: true, 
    event_logs: false 
  })
  const [query, setQuery] = useState('')

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

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    return mockExports.filter((r) => !q || (r.series + ' ' + r.event).toLowerCase().includes(q))
  }, [query])

  function toggleType(k: keyof typeof types) {
    setTypes((t) => ({ ...t, [k]: !t[k] }))
  }

  const handleExport = async (full: boolean = false) => {
    if (!selectedEventId) {
      toast.error('Selecciona un evento primero')
      return
    }

    try {
      const filePath = await save({
        filters: [{ name: 'Excel', extensions: ['xlsx'] }],
        defaultPath: `Resultados_Evento_${selectedEventId}.xlsx`
      })

      if (!filePath) return

      const options = full ? {
        overview: true,
        teams: true,
        run_order: true,
        standings: true,
        payoffs: true,
        event_logs: true,
        file_path: filePath
      } : {
        ...types,
        file_path: filePath
      }

      await exportEvent(parseInt(selectedEventId), options)
      toast.success('Exportación completada exitosamente')
    } catch (e) {
      console.error(e)
      toast.error('Error al exportar el archivo')
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
            <Button variant="ghost" onClick={() => setSelectedSeriesId(selectedSeriesId)}>Refrescar</Button>
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
              <button type="button" onClick={() => toggleType('standings')} className={`text-left rounded-md p-4 border ${types.standings ? 'border-orange-200 bg-orange-50' : 'border-border bg-card'}`}>
                <div className="font-medium">Resultados globales</div>
                <div className="text-xs text-muted-foreground">Standings completos del evento</div>
              </button>

              <button type="button" onClick={() => toggleType('run_order')} className={`text-left rounded-md p-4 border ${types.run_order ? 'border-orange-200 bg-orange-50' : 'border-border bg-card'}`}>
                <div className="font-medium">Resultados por ronda</div>
                <div className="text-xs text-muted-foreground">Detalle de cada ronda</div>
              </button>

              <button type="button" onClick={() => toggleType('teams')} className={`text-left rounded-md p-4 border ${types.teams ? 'border-orange-200 bg-orange-50' : 'border-border bg-card'}`}>
                <div className="font-medium">Equipos</div>
                <div className="text-xs text-muted-foreground">Lista de equipos participantes</div>
              </button>

              <button type="button" onClick={() => toggleType('payoffs')} className={`text-left rounded-md p-4 border ${types.payoffs ? 'border-orange-200 bg-orange-50' : 'border-border bg-card'}`}>
                <div className="font-medium">Payoffs</div>
                <div className="text-xs text-muted-foreground">Distribución de premios</div>
              </button>

              <button type="button" onClick={() => toggleType('event_logs')} className={`text-left rounded-md p-4 border ${types.event_logs ? 'border-orange-200 bg-orange-50' : 'border-border bg-card'}`}>
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
                {filtered.map((r) => (
                  <TableRow key={r.id}>
                    <TableCell className="text-sm text-muted-foreground">{r.date}</TableCell>
                    <TableCell>{r.series}</TableCell>
                    <TableCell>{r.event}</TableCell>
                    <TableCell>{r.type}</TableCell>
                    <TableCell className="text-sm text-muted-foreground">{r.size}</TableCell>
                    <TableCell>
                      {r.status === 'Completado' && <Badge className="bg-emerald-50 text-emerald-700 border-emerald-200">Completado</Badge>}
                    </TableCell>
                    <TableCell className="text-right text-orange-600">Re-exportar</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        </div>
      </div>
    </div>
  )
}
