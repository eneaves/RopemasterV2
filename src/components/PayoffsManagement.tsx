import { useState, useEffect } from 'react'
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
import { getSeries, getEvents, getStandings, getPayoutBreakdown } from '../lib/api'
import { toast } from 'sonner'
import type { Series, Event } from '../types'

interface PayoffEntry {
  id: string
  pos: number
  header: string
  heeler: string
  amount: number
  time: number | null
  date: string
  status: 'Calculado' | 'Pagado' // Simplified for now
}

function formatCurrency(n: number) {
  return new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', maximumFractionDigits: 0 }).format(n)
}

export function PayoffsManagement() {
  const [seriesList, setSeriesList] = useState<Series[]>([])
  const [eventsList, setEventsList] = useState<Event[]>([])
  const [selectedSeriesId, setSelectedSeriesId] = useState<string | null>(null)
  const [selectedEventId, setSelectedEventId] = useState<string | null>(null)

  const [payoffEntries, setPayoffEntries] = useState<PayoffEntry[]>([])
  const [financials, setFinancials] = useState({ totalPot: 0, deductions: 0, netPot: 0 })
  
  const [query, setQuery] = useState('')
  const [statusFilter, setStatusFilter] = useState('Todos los Estados')

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

  // Load payoffs and standings
  useEffect(() => {
    if (selectedEventId) {
      const event = eventsList.find(e => e.id.toString() === selectedEventId)
      const eventDate = event ? new Date(event.date).toLocaleDateString('es-MX', { day: 'numeric', month: 'short', year: 'numeric' }) : '-'

      Promise.all([
        getStandings(parseInt(selectedEventId)),
        getPayoutBreakdown(parseInt(selectedEventId))
      ]).then(([standingsData, payoutData]) => {
        setFinancials({
          totalPot: payoutData.total_pot,
          deductions: payoutData.deductions,
          netPot: payoutData.net_pot
        })

        const payoutMap = new Map<number, number>()
        payoutData.payouts.forEach((p: any) => payoutMap.set(p.place, p.amount))

        // Filter only winners (those who have a payout)
        const winners = standingsData
          .filter((s: any) => payoutMap.has(s.rank))
          .map((s: any) => ({
            id: `payoff-${s.team_id}`,
            pos: s.rank,
            header: s.header_name,
            heeler: s.heeler_name,
            amount: payoutMap.get(s.rank) || 0,
            time: s.total_time,
            date: eventDate,
            status: 'Calculado' as const
          }))
        
        setPayoffEntries(winners)
      }).catch(e => {
        console.warn('Error loading payoffs (likely no teams/results yet):', e)
        // Don't show error toast, just clear data
        setPayoffEntries([])
        setFinancials({ totalPot: 0, deductions: 0, netPot: 0 })
      })
    } else {
      setPayoffEntries([])
      setFinancials({ totalPot: 0, deductions: 0, netPot: 0 })
    }
  }, [selectedEventId, eventsList])

  const selectedSeries = seriesList.find(s => s.id.toString() === selectedSeriesId)
  const selectedEvent = eventsList.find(e => e.id.toString() === selectedEventId)

  const filtered = payoffEntries.filter((r) => {
    const q = query.trim().toLowerCase()
    if (q && !(r.header + ' ' + r.heeler + ' ' + r.pos).toLowerCase().includes(q)) return false
    if (statusFilter !== 'Todos los Estados' && r.status !== statusFilter) return false
    return true
  })

  const handleRecalculate = () => {
    // In a real scenario, this might trigger a backend recalculation if parameters changed
    // For now, we just refresh the data
    const current = selectedEventId
    setSelectedEventId(null)
    setTimeout(() => setSelectedEventId(current), 50)
    toast.success('Payoffs recalculados')
  }

  return (
    <div className="p-6 h-full flex flex-col overflow-hidden">
      <div className="flex-1 overflow-y-auto">
        <div className="mb-6 flex items-start justify-between">
          <div>
            <h1 className="text-2xl font-semibold text-foreground">Gestión de Payoffs</h1>
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

            <Button variant="ghost" className="rounded-md">Exportar Reporte</Button>
            <Button className="bg-orange-500 hover:bg-orange-600 text-white rounded-md" onClick={handleRecalculate} disabled={!selectedEventId}>
              Recalcular Payoffs
            </Button>
          </div>
        </div>

        {/* KPI cards */}
        <div className="grid grid-cols-1 gap-4 md:grid-cols-4 mb-6">
          <div className="bg-green-50 border border-green-100 rounded-md p-4">
            <div className="text-sm text-green-700">Total Pot</div>
            <div className="text-2xl font-semibold text-green-800">{formatCurrency(financials.totalPot)}</div>
            <div className="text-xs text-muted-foreground">Del evento actual</div>
          </div>

          <div className="bg-white border border-border rounded-md p-4">
            <div className="text-sm text-muted-foreground">Deducciones</div>
            <div className="text-2xl font-semibold">{formatCurrency(financials.deductions)}</div>
            <div className="text-xs text-muted-foreground">Fees y gastos</div>
          </div>

          <div className="bg-orange-50 border border-orange-100 rounded-md p-4">
            <div className="text-sm text-orange-700">Net Pot</div>
            <div className="text-2xl font-semibold text-orange-800">{formatCurrency(financials.netPot)}</div>
            <div className="text-xs text-muted-foreground">A distribuir</div>
          </div>

          <div className="bg-blue-50 border border-blue-100 rounded-md p-4">
            <div className="text-sm text-blue-700">Total Ganadores</div>
            <div className="text-2xl font-semibold text-blue-800">{payoffEntries.length}</div>
            <div className="text-xs text-muted-foreground">Equipos pagados</div>
          </div>
        </div>

        {/* History panel */}
        <div className="bg-card border border-border rounded-xl p-4">
          <div className="mb-4">
            <h2 className="text-lg font-medium">Desglose de Premios</h2>
            <p className="text-sm text-muted-foreground">Distribución de premios por posición</p>
          </div>

          <div className="mb-4 flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
            <div className="flex items-center gap-3 w-full md:w-1/2">
              <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Buscar por equipo o posición..." />
            </div>

            <div className="flex items-center gap-3">
              <Select value={statusFilter} onValueChange={setStatusFilter}>
                <SelectTrigger className="w-[180px]">
                  <SelectValue placeholder="Estado" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="Todos los Estados">Todos los Estados</SelectItem>
                  <SelectItem value="Calculado">Calculado</SelectItem>
                  <SelectItem value="Pagado">Pagado</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          {/* Table */}
          <div className="mt-2">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Posición</TableHead>
                  <TableHead>Equipo</TableHead>
                  <TableHead>Monto</TableHead>
                  <TableHead>Tiempo Total</TableHead>
                  <TableHead>Fecha</TableHead>
                  <TableHead>Estado</TableHead>
                  <TableHead>Acciones</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {filtered.length > 0 ? (
                  filtered.map((r) => (
                    <TableRow key={r.id}>
                      <TableCell className="w-24">
                        <span className={`inline-flex items-center rounded-md px-3 py-1 text-sm text-white ${
                          r.pos === 1 ? 'bg-orange-500' : 
                          r.pos === 2 ? 'bg-gray-400' : 
                          r.pos === 3 ? 'bg-orange-700' : 'bg-slate-400'
                        }`}>#{r.pos}</span>
                      </TableCell>
                      <TableCell>
                        <div className="text-foreground">{r.header}</div>
                        <div className="text-muted-foreground text-sm">&amp; {r.heeler}</div>
                      </TableCell>
                      <TableCell className="font-medium text-green-700">{formatCurrency(r.amount)}</TableCell>
                      <TableCell className="text-muted-foreground">{r.time ? `${r.time.toFixed(2)}s` : 'NT'}</TableCell>
                      <TableCell className="text-muted-foreground">{r.date}</TableCell>
                      <TableCell>
                        <Badge className="bg-emerald-50 text-emerald-700 border-emerald-200">{r.status}</Badge>
                      </TableCell>
                      <TableCell className="text-right">
                        <Button variant="ghost" size="sm">•••</Button>
                      </TableCell>
                    </TableRow>
                  ))
                ) : (
                  <TableRow>
                    <TableCell colSpan={7} className="text-center py-8 text-muted-foreground">No hay payoffs para mostrar</TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </div>
        </div>
      </div>
    </div>
  )
}
