import { useMemo, useState, useEffect } from 'react'
import { Button } from './ui/button'
import { Input } from './ui/input'
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
import { MetricsPanel } from './MetricsPanel'
import { getSeries, getEvents, getStandings, getPayoutBreakdown, exportEvent, getDashboardStats } from '../lib/api'
import { save } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'
import type { Series, Event } from '../types'

interface Standing {
  rank: number
  team_id: number
  header_name: string
  heeler_name: string
  total_time: number | null
  completed_runs: number
  nt_cnt: number
  dq_cnt: number
  avg_time: number | null
  best_time: number | null
  payoff?: number
  status: 'Calificado' | 'Penal' | 'No Time' | 'DQ'
}

function formatCurrency(n: number) {
  return new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', maximumFractionDigits: 0 }).format(n)
}

export function ResultsManagement() {
  const [seriesList, setSeriesList] = useState<Series[]>([])
  const [eventsList, setEventsList] = useState<Event[]>([])
  const [selectedSeriesId, setSelectedSeriesId] = useState<string | null>(null)
  const [selectedEventId, setSelectedEventId] = useState<string | null>(null)
  
  const [standings, setStandings] = useState<Standing[]>([])
  const [dashboardStats, setDashboardStats] = useState<any>(null)
  const [filterQuery, setFilterQuery] = useState('')
  const [placeFilter, setPlaceFilter] = useState('Todos')

  // Load initial data
  useEffect(() => {
    getSeries().then((data) => {
      setSeriesList(data)
      const active = data.find((s) => s.status === 'active')
      if (active) setSelectedSeriesId(active.id.toString())
    })

    getDashboardStats().then(setDashboardStats).catch(console.error)
  }, [])

  // Load events when series changes
  useEffect(() => {
    if (selectedSeriesId) {
      getEvents(parseInt(selectedSeriesId)).then((data) => {
        setEventsList(data)
        // Select first active event if available
        const active = data.find((e) => e.status === 'active' || e.status === 'completed')
        if (active) setSelectedEventId(active.id.toString())
        else setSelectedEventId(null)
      })
    } else {
      setEventsList([])
    }
  }, [selectedSeriesId])

  // Load standings when event changes
  useEffect(() => {
    if (selectedEventId) {
      Promise.all([
        getStandings(parseInt(selectedEventId)),
        getPayoutBreakdown(parseInt(selectedEventId))
      ]).then(([standingsData, payoutData]) => {
        // Map payouts by place
        const payoutMap = new Map<number, number>()
        payoutData.payouts.forEach((p: any) => payoutMap.set(p.place, p.amount))

        const mapped: Standing[] = standingsData.map((s: any) => {
          let status: Standing['status'] = 'Calificado'
          if (s.dq_cnt > 0) status = 'DQ'
          else if (s.nt_cnt > 0) status = 'No Time'
          // Logic for 'Penal' is tricky without run details, assuming clean runs for now unless we check penalties
          
          return {
            ...s,
            payoff: payoutMap.get(s.rank) || 0,
            status
          }
        })
        setStandings(mapped)
      }).catch(e => {
        console.error(e)
        toast.error('Error al cargar resultados')
      })
    } else {
      setStandings([])
    }
  }, [selectedEventId])

  const selectedSeries = seriesList.find(s => s.id.toString() === selectedSeriesId)
  const selectedEvent = eventsList.find(e => e.id.toString() === selectedEventId)

  const totals = useMemo(() => {
    const totalRuns = standings.reduce((acc, s) => acc + s.completed_runs, 0)
    const validTimes = standings.filter(s => s.total_time !== null).map(s => s.total_time as number)
    const avg = validTimes.length > 0 
      ? validTimes.reduce((a, b) => a + b, 0) / validTimes.length 
      : 0
    const totalPayoffs = standings.reduce((acc, s) => acc + (s.payoff || 0), 0)
    const qualified = standings.filter((s) => s.status === 'Calificado').length
    return { totalRuns, avg: avg.toFixed(2), totalPayoffs, qualified }
  }, [standings])

  const filtered = standings.filter((s) => {
    const searchStr = `${s.header_name} ${s.heeler_name}`.toLowerCase()
    if (filterQuery && !searchStr.includes(filterQuery.toLowerCase())) return false
    if (placeFilter !== 'Todos' && String(s.rank) !== placeFilter) return false
    return true
  })

  const handleExport = async () => {
    if (!selectedEventId) return
    try {
      const filePath = await save({
        filters: [{ name: 'Excel', extensions: ['xlsx'] }]
      })
      if (filePath) {
        await exportEvent(parseInt(selectedEventId), {
          overview: true,
          teams: true,
          run_order: true,
          standings: true,
          payoffs: true,
          event_logs: true,
          file_path: filePath
        })
        toast.success('Resultados exportados')
      }
    } catch (e) {
      console.error(e)
      toast.error('Error al exportar')
    }
  }

  const handleRefresh = () => {
    // Trigger re-fetch by toggling selection or just calling the effect logic
    // Simplest is to just clear and reset ID if we want to trigger effect, 
    // but better to extract fetch logic. For now, just re-set the ID to itself? No, effect deps check value.
    // We can just force update.
    const current = selectedEventId
    setSelectedEventId(null)
    setTimeout(() => setSelectedEventId(current), 50)
  }

  return (
    <div className="flex-1 h-full flex overflow-hidden bg-background">
      {/* Main column (center) */}
      <div className="flex-1 overflow-y-auto">
        <div className="p-6 max-w-full">
          <div className="mb-6 flex items-start justify-between">
            <div>
              <h1 className="text-2xl font-semibold text-foreground">Resultados del Evento</h1>
              <p className="text-sm text-muted-foreground">
                {selectedSeries ? `Serie: ${selectedSeries.name}` : 'Selecciona una serie'} — 
                {selectedEvent ? ` Evento: ${selectedEvent.name}` : ' Selecciona un evento'}
              </p>
            </div>

            <div className="flex items-center gap-3">
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

              <Button variant="ghost" onClick={handleRefresh}>Refrescar</Button>
              <Button className="bg-orange-500 hover:bg-orange-600 text-white" onClick={handleExport} disabled={!selectedEventId}>
                Exportar Resultados
              </Button>
            </div>
          </div>

          {/* Podium cards */}
          {standings.length > 0 ? (
            <div className="mb-6 grid grid-cols-1 md:grid-cols-3 gap-6 items-end">
              {/* 2nd Place */}
              <div className="bg-card rounded-xl p-6 shadow-sm text-center order-2 md:order-1">
                <div className="text-muted-foreground mb-4 text-4xl">🥈</div>
                {standings[1] ? (
                  <>
                    <div className="font-medium text-lg">{standings[1].header_name}</div>
                    <div className="text-sm text-muted-foreground">& {standings[1].heeler_name}</div>
                    <div className="mt-4 bg-muted rounded-md p-4">
                      <span className="text-2xl font-bold">{standings[1].total_time?.toFixed(2)}s</span>
                      <br/><span className="text-xs text-muted-foreground">Tiempo Total</span>
                    </div>
                    <div className="mt-3 text-lg text-orange-700 font-medium">💰 {formatCurrency(standings[1].payoff || 0)}</div>
                  </>
                ) : <div className="text-muted-foreground py-8">Sin datos</div>}
              </div>

              {/* 1st Place */}
              <div className="bg-orange-500 text-white rounded-xl p-8 shadow-lg transform scale-105 text-center order-1 md:order-2 z-10">
                <div className="text-5xl mb-2">🏆</div>
                {standings[0] ? (
                  <>
                    <div className="mt-3 font-semibold text-xl">{standings[0].header_name}</div>
                    <div className="text-sm opacity-90">& {standings[0].heeler_name}</div>
                    <div className="mt-6 bg-white/20 rounded-md p-6 text-3xl font-bold">{standings[0].total_time?.toFixed(2)}s</div>
                    <div className="mt-3 text-sm">Rondas: {standings[0].completed_runs}</div>
                    <div className="mt-2 text-2xl font-bold">{formatCurrency(standings[0].payoff || 0)}</div>
                  </>
                ) : <div className="opacity-80 py-8">Sin datos</div>}
              </div>

              {/* 3rd Place */}
              <div className="bg-card rounded-xl p-6 shadow-sm text-center order-3">
                <div className="text-muted-foreground mb-4 text-4xl">🥉</div>
                {standings[2] ? (
                  <>
                    <div className="font-medium text-lg">{standings[2].header_name}</div>
                    <div className="text-sm text-muted-foreground">& {standings[2].heeler_name}</div>
                    <div className="mt-4 bg-muted rounded-md p-4">
                      <span className="text-2xl font-bold">{standings[2].total_time?.toFixed(2)}s</span>
                      <br/><span className="text-xs text-muted-foreground">Tiempo Total</span>
                    </div>
                    <div className="mt-3 text-lg text-orange-700 font-medium">💰 {formatCurrency(standings[2].payoff || 0)}</div>
                  </>
                ) : <div className="text-muted-foreground py-8">Sin datos</div>}
              </div>
            </div>
          ) : (
            <div className="mb-6 p-12 text-center border border-dashed border-border rounded-xl">
              <p className="text-muted-foreground">Selecciona un evento para ver los resultados</p>
            </div>
          )}

          {/* KPI cards */}
          <div className="grid grid-cols-1 gap-4 md:grid-cols-4 mb-6">
            <div className="bg-card border border-border rounded-md p-4 text-center">
              <div className="text-sm text-muted-foreground">Total de Corridas</div>
              <div className="text-2xl font-semibold">{totals.totalRuns}</div>
            </div>
            <div className="bg-card border border-border rounded-md p-4 text-center">
              <div className="text-sm text-muted-foreground">Promedio General</div>
              <div className="text-2xl font-semibold">{totals.avg}s</div>
            </div>
            <div className="bg-card border border-border rounded-md p-4 text-center">
              <div className="text-sm text-muted-foreground">Total Payoffs</div>
              <div className="text-2xl font-semibold">{formatCurrency(totals.totalPayoffs)}</div>
            </div>
            <div className="bg-card border border-border rounded-md p-4 text-center">
              <div className="text-sm text-muted-foreground">Equipos Calificados</div>
              <div className="text-2xl font-semibold">{totals.qualified}</div>
            </div>
          </div>

          {/* Filters */}
          <div className="bg-card border border-border rounded-xl p-4 mb-4">
            <div className="flex flex-col md:flex-row gap-3 items-center">
              <Input placeholder="Buscar equipo o roper..." value={filterQuery} onChange={(e) => setFilterQuery(e.target.value)} className="max-w-sm" />
              <Select value={placeFilter} onValueChange={setPlaceFilter}>
                <SelectTrigger className="w-[150px]">
                  <SelectValue placeholder="Lugar" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="Todos">Todos</SelectItem>
                  {[...Array(10)].map((_, i) => (
                    <SelectItem key={i} value={String(i + 1)}>#{i + 1}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          {/* Table */}
          <div className="bg-card border border-border rounded-xl p-4 mb-6">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Pos</TableHead>
                  <TableHead>Header</TableHead>
                  <TableHead>Heeler</TableHead>
                  <TableHead>Total Rondas</TableHead>
                  <TableHead>Tiempo Total</TableHead>
                  <TableHead>Promedio</TableHead>
                  <TableHead>Estado</TableHead>
                  <TableHead className="text-right">Payoff</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {filtered.length > 0 ? (
                  filtered.map((r) => (
                    <TableRow key={r.team_id} className={r.rank === 1 ? 'bg-orange-50' : ''}>
                      <TableCell>
                        <span className={`inline-flex items-center justify-center rounded-full px-2 py-1 text-sm ${
                          r.rank === 1 ? 'bg-orange-500 text-white' : 
                          r.rank === 2 ? 'bg-gray-400 text-white' :
                          r.rank === 3 ? 'bg-amber-600 text-white' : 'bg-muted text-muted-foreground'
                        }`}>#{r.rank}</span>
                      </TableCell>
                      <TableCell>{r.header_name}</TableCell>
                      <TableCell>{r.heeler_name}</TableCell>
                      <TableCell>{r.completed_runs}</TableCell>
                      <TableCell>{r.total_time !== null ? `${r.total_time.toFixed(2)}s` : 'NT'}</TableCell>
                      <TableCell>{r.avg_time !== null ? `${r.avg_time.toFixed(2)}s` : '—'}</TableCell>
                      <TableCell>
                        {r.status === 'Calificado' && <Badge className="bg-emerald-50 text-emerald-700 border-emerald-200">Calificado</Badge>}
                        {r.status === 'Penal' && <Badge className="bg-orange-50 text-orange-700 border-orange-200">Penal</Badge>}
                        {r.status === 'No Time' && <Badge className="bg-red-50 text-red-700 border-red-200">No Time</Badge>}
                        {r.status === 'DQ' && <Badge className="bg-red-50 text-red-700 border-red-200">DQ</Badge>}
                      </TableCell>
                      <TableCell className="text-right font-medium text-orange-700">
                        {r.payoff ? formatCurrency(r.payoff) : '—'}
                      </TableCell>
                    </TableRow>
                  ))
                ) : (
                  <TableRow>
                    <TableCell colSpan={8} className="text-center py-8 text-muted-foreground">
                      No se encontraron resultados
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </div>
        </div>
      </div>

      {/* Right metrics panel (scrollable) */}
      <MetricsPanel stats={dashboardStats} />
    </div>
  )
}
