import { useState, useMemo, useEffect } from 'react'
import { Search, Download, RefreshCw } from 'lucide-react'
import { Badge } from './ui/badge'
import { Button } from './ui/button'
import { Input } from './ui/input'
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from './ui/select'
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from './ui/table'
import { getStandings, getPayoutBreakdown, exportEvent } from '../lib/api'
import { save } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'

interface StandingsTabProps {
  event: any
}

interface Standing {
  rank: number
  teamId: number
  header: string
  heeler: string
  avgTime: number
  totalTime: number | null
  qualifiedRuns: number
  totalRuns: number // qualified + nt + dq
  ntCount: number
  dqCount: number
  isQualified: boolean
  payoff?: number
  status: 'Calificado' | 'Penal' | 'No Time' | 'DQ'
}

function formatCurrency(n: number) {
  return new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', maximumFractionDigits: 0 }).format(n)
}

export function StandingsTab({ event }: StandingsTabProps) {
  const [standings, setStandings] = useState<Standing[]>([])
  const [filterQuery, setFilterQuery] = useState('')
  const [placeFilter, setPlaceFilter] = useState('Todos')
  const [loading, setLoading] = useState(false)

  const fetchData = async () => {
    if (!event?.id) return
    setLoading(true)
    try {
      const [standingsData, payoutData] = await Promise.all([
        getStandings(Number(event.id)),
        getPayoutBreakdown(Number(event.id)).catch(() => ({ payouts: [] })) // Handle case where payoffs not calculated
      ])

      const payoutMap = new Map<number, number>()
      if (payoutData?.payouts) {
        payoutData.payouts.forEach((p: any) => payoutMap.set(p.place, p.amount))
      }

      const mapped: Standing[] = standingsData.map((s: any) => {
        let status: Standing['status'] = 'Calificado'
        if (s.dq_cnt > 0) status = 'DQ'
        else if (s.nt_cnt > 0) status = 'No Time'
        
        return {
          rank: s.rank,
          teamId: s.team_id,
          header: s.header_name,
          heeler: s.heeler_name,
          avgTime: s.avg_time || 0,
          totalTime: s.total_time,
          qualifiedRuns: s.completed_runs,
          totalRuns: s.completed_runs + s.nt_cnt + s.dq_cnt,
          ntCount: s.nt_cnt,
          dqCount: s.dq_cnt,
          isQualified: s.nt_cnt === 0 && s.dq_cnt === 0,
          payoff: payoutMap.get(s.rank) || 0,
          status
        }
      })
      setStandings(mapped)
    } catch (err) {
      console.error('Error fetching standings:', err)
      toast.error('Error al cargar standings')
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    fetchData()
  }, [event?.id])

  const totals = useMemo(() => {
    const totalRuns = standings.reduce((acc, s) => acc + s.qualifiedRuns, 0)
    const validTimes = standings.filter(s => s.totalTime !== null && s.totalTime > 0).map(s => s.totalTime as number)
    const avg = validTimes.length > 0
      ? validTimes.reduce((a, b) => a + b, 0) / validTimes.length
      : 0
    const totalPayoffs = standings.reduce((acc, s) => acc + (s.payoff || 0), 0)
    const qualified = standings.filter((s) => s.isQualified).length
    return { totalRuns, avg: avg.toFixed(2), totalPayoffs, qualified }
  }, [standings])

  const filteredStandings = useMemo(() => {
    return standings.filter((s) => {
      const searchStr = `${s.header} ${s.heeler}`.toLowerCase()
      if (filterQuery && !searchStr.includes(filterQuery.toLowerCase())) return false
      if (placeFilter !== 'Todos' && String(s.rank) !== placeFilter) return false
      return true
    })
  }, [filterQuery, placeFilter, standings])
  
  const handleExport = async () => {
    if (!event?.id) return
    try {
      const filePath = await save({
        filters: [{ name: 'Excel', extensions: ['xlsx'] }]
      })
      if (filePath) {
        await exportEvent(parseInt(event.id), {
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

  return (
    <div className="space-y-6 h-full flex flex-col">
      {/* Header */}
      <div className="flex flex-col md:flex-row justify-between items-start md:items-center gap-4">
        <div>
          <h2 className="text-2xl font-bold text-foreground flex items-center gap-2">
            Standings & Resultados
            {loading && <RefreshCw className="w-4 h-4 animate-spin text-muted-foreground" />}
          </h2>
          <p className="text-muted-foreground">Clasificación en tiempo real y desglose de premios</p>
        </div>
        <div className="flex items-center gap-2">
           <Button variant="outline" onClick={fetchData} className="h-10">
              <RefreshCw className="w-4 h-4 mr-2" />
              Actualizar
           </Button>
           <Button className="bg-emerald-600 hover:bg-emerald-700 text-white h-10" onClick={handleExport}>
              <Download className="w-4 h-4 mr-2" />
              Exportar Excel
           </Button>
        </div>
      </div>

      {/* Podium Cards */}
      {standings.length > 0 && (
         <div className="grid grid-cols-1 md:grid-cols-3 gap-6 items-end mb-4">
              {/* 2nd Place */}
              <div className="bg-card rounded-xl border border-border p-6 shadow-sm text-center order-2 md:order-1 relative">
                <div className="absolute -top-4 left-1/2 -translate-x-1/2 bg-gray-100 text-gray-600 border border-gray-200 w-8 h-8 rounded-full flex items-center justify-center font-bold shadow-sm">2</div>
                <div className="text-4xl mb-3">🥈</div>
                {standings[1] ? (
                  <>
                    <div className="font-semibold text-lg text-foreground truncate">{standings[1].header}</div>
                    <div className="text-sm text-muted-foreground truncate">& {standings[1].heeler}</div>
                    <div className="mt-4 bg-muted/50 rounded-lg p-3">
                      <span className="text-2xl font-bold text-foreground tabular-nums">{standings[1].totalTime?.toFixed(2)}s</span>
                      <p className="text-xs text-muted-foreground mt-1">Tiempo Total</p>
                    </div>
                    {standings[1].payoff ? (
                        <div className="mt-3 text-lg text-emerald-600 font-bold">{formatCurrency(standings[1].payoff)}</div>
                    ) : null}
                  </>
                ) : <div className="text-muted-foreground py-6 text-sm">Sin clasificado</div>}
              </div>

              {/* 1st Place */}
              <div className="bg-card rounded-xl border-2 border-amber-400 p-8 shadow-md text-center order-1 md:order-2 transform md:-translate-y-2 relative z-10">
                <div className="absolute -top-5 left-1/2 -translate-x-1/2 bg-amber-500 text-white border-4 border-background w-10 h-10 rounded-full flex items-center justify-center font-bold shadow-sm text-lg">1</div>
                <div className="text-5xl mb-4">🏆</div>
                {standings[0] ? (
                  <>
                    <div className="font-bold text-xl text-foreground truncate">{standings[0].header}</div>
                    <div className="text-base text-muted-foreground truncate mb-4">& {standings[0].heeler}</div>
                    <div className="bg-amber-50 rounded-xl p-4 border border-amber-100">
                      <span className="text-4xl font-extrabold text-amber-600 tabular-nums">{standings[0].totalTime?.toFixed(2)}s</span>
                      <p className="text-xs text-amber-600/70 font-medium mt-1 uppercase tracking-wide">Tiempo Campeón</p>
                    </div>
                    <div className="mt-4 text-sm text-muted-foreground font-medium">
                        Rondas Completadas: {standings[0].qualifiedRuns}
                    </div>
                    {standings[0].payoff ? (
                        <div className="mt-2 text-2xl font-bold text-emerald-600">{formatCurrency(standings[0].payoff)}</div>
                    ) : null}
                  </>
                ) : <div className="text-muted-foreground py-8">Sin resultados</div>}
              </div>

              {/* 3rd Place */}
              <div className="bg-card rounded-xl border border-border p-6 shadow-sm text-center order-3 relative">
                <div className="absolute -top-4 left-1/2 -translate-x-1/2 bg-orange-50 text-orange-700 border border-orange-200 w-8 h-8 rounded-full flex items-center justify-center font-bold shadow-sm">3</div>
                <div className="text-4xl mb-3">🥉</div>
                {standings[2] ? (
                  <>
                    <div className="font-semibold text-lg text-foreground truncate">{standings[2].header}</div>
                    <div className="text-sm text-muted-foreground truncate">& {standings[2].heeler}</div>
                    <div className="mt-4 bg-muted/50 rounded-lg p-3">
                      <span className="text-2xl font-bold text-foreground tabular-nums">{standings[2].totalTime?.toFixed(2)}s</span>
                      <p className="text-xs text-muted-foreground mt-1">Tiempo Total</p>
                    </div>
                     {standings[2].payoff ? (
                        <div className="mt-3 text-lg text-emerald-600 font-bold">{formatCurrency(standings[2].payoff)}</div>
                    ) : null}
                  </>
                ) : <div className="text-muted-foreground py-6 text-sm">Sin clasificado</div>}
              </div>
         </div>
      )}

      {/* KPI Stats */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
        <div className="bg-card border border-border rounded-xl p-4 flex flex-col items-center justify-center text-center shadow-sm hover:border-primary/50 transition-colors">
          <span className="text-xs font-medium text-muted-foreground uppercase tracking-wider mb-1">Total Corridas</span>
          <span className="text-2xl font-bold text-foreground">{totals.totalRuns}</span>
        </div>
        <div className="bg-card border border-border rounded-xl p-4 flex flex-col items-center justify-center text-center shadow-sm hover:border-primary/50 transition-colors">
          <span className="text-xs font-medium text-muted-foreground uppercase tracking-wider mb-1">Promedio General</span>
          <span className="text-2xl font-bold text-foreground tabular-nums">{totals.avg}s</span>
        </div>
        <div className="bg-card border border-border rounded-xl p-4 flex flex-col items-center justify-center text-center shadow-sm hover:border-primary/50 transition-colors">
          <span className="text-xs font-medium text-muted-foreground uppercase tracking-wider mb-1">Proyección Premios</span>
          <span className="text-2xl font-bold text-emerald-600 tabular-nums">{formatCurrency(totals.totalPayoffs)}</span>
        </div>
        <div className="bg-card border border-border rounded-xl p-4 flex flex-col items-center justify-center text-center shadow-sm hover:border-primary/50 transition-colors">
          <span className="text-xs font-medium text-muted-foreground uppercase tracking-wider mb-1">Equipos Calificados</span>
          <span className="text-2xl font-bold text-foreground">{totals.qualified}</span>
        </div>
      </div>

      {/* Filters & Table */}
      <div className="space-y-4 flex-1 flex flex-col min-h-0">
          <div className="flex flex-col sm:flex-row gap-3">
            <div className="relative flex-1">
                <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground" />
                <Input 
                    placeholder="Buscar equipo, header o heeler..." 
                    value={filterQuery} 
                    onChange={(e) => setFilterQuery(e.target.value)} 
                    className="pl-9 bg-card border-border h-10 rounded-xl"
                />
            </div>
            <Select value={placeFilter} onValueChange={setPlaceFilter}>
                <SelectTrigger className="w-full sm:w-[160px] bg-card border-border h-10 rounded-xl">
                  <SelectValue placeholder="Filtrar por Lugar" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="Todos">Todos los lugares</SelectItem>
                  {[...Array(Math.min(20, standings.length))].map((_, i) => (
                    <SelectItem key={i} value={String(i + 1)}>Puesto #{i + 1}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
          </div>

          <div className="bg-card border border-border rounded-xl flex-1 overflow-hidden flex flex-col shadow-sm">
            <div className="overflow-auto flex-1">
                <Table>
                <TableHeader className="sticky top-0 bg-muted/50 z-10 backdrop-blur-sm">
                    <TableRow className="hover:bg-transparent border-border">
                    <TableHead className="w-20 text-center font-medium">Rank</TableHead>
                    <TableHead className="w-24 font-medium">Team ID</TableHead>
                    <TableHead className="font-medium">Header</TableHead>
                    <TableHead className="font-medium">Heeler</TableHead>
                    <TableHead className="text-center font-medium hidden md:table-cell">Rondas</TableHead>
                    <TableHead className="text-right font-medium">Tiempo Total</TableHead>
                    <TableHead className="text-right font-medium hidden sm:table-cell">Promedio</TableHead>
                    <TableHead className="text-center font-medium">Estado</TableHead>
                    <TableHead className="text-right font-medium">Premio</TableHead>
                    </TableRow>
                </TableHeader>
                <TableBody>
                    {filteredStandings.length > 0 ? (
                    filteredStandings.map((s) => (
                        <TableRow key={s.teamId} className={`hover:bg-muted/30 border-border/50 ${s.rank <= 3 ? 'bg-muted/10' : ''}`}>
                        <TableCell className="text-center">
                            <Badge 
                                variant="outline" 
                                className={`
                                    w-8 h-8 rounded-full p-0 flex items-center justify-center border
                                    ${s.rank === 1 ? 'bg-amber-100 text-amber-700 border-amber-200' : ''}
                                    ${s.rank === 2 ? 'bg-gray-100 text-gray-700 border-gray-200' : ''}
                                    ${s.rank === 3 ? 'bg-orange-100 text-orange-700 border-orange-200' : ''}
                                    ${s.rank > 3 ? 'border-border text-muted-foreground' : ''}
                                `}
                            >
                                {s.rank}
                            </Badge>
                        </TableCell>
                        <TableCell className="text-muted-foreground font-mono text-xs">#{s.teamId}</TableCell>
                        <TableCell className="font-medium text-foreground">{s.header}</TableCell>
                        <TableCell className="font-medium text-foreground">{s.heeler}</TableCell>
                        <TableCell className="text-center hidden md:table-cell">
                            <Badge variant="secondary" className="font-normal">{s.qualifiedRuns} / {s.totalRuns}</Badge>
                        </TableCell>
                        <TableCell className="text-right font-mono font-medium text-foreground">
                            {s.totalTime !== null ? `${s.totalTime.toFixed(2)}s` : <span className="text-muted-foreground">-</span>}
                        </TableCell>
                        <TableCell className="text-right font-mono text-muted-foreground hidden sm:table-cell">
                             {s.avgTime !== null ? `${s.avgTime.toFixed(2)}s` : '-'}
                        </TableCell>
                        <TableCell className="text-center">
                            {s.status === 'Calificado' && <Badge className="bg-emerald-50 text-emerald-700 border-emerald-200 hover:bg-emerald-100 text-xs">OK</Badge>}
                            {s.status === 'DQ' && <Badge className="bg-red-50 text-red-700 border-red-200 hover:bg-red-100 text-xs">DQ</Badge>}
                            {s.status === 'No Time' && <Badge className="bg-red-50 text-red-700 border-red-200 hover:bg-red-100 text-xs">NT</Badge>}
                        </TableCell>
                        <TableCell className="text-right font-semibold text-emerald-600">
                             {s.payoff ? formatCurrency(s.payoff) : <span className="text-muted-foreground/30">—</span>}
                        </TableCell>
                        </TableRow>
                    ))
                    ) : (
                    <TableRow>
                        <TableCell colSpan={9} className="h-32 text-center text-muted-foreground">
                            <div className="flex flex-col items-center justify-center gap-2">
                                <Search className="w-8 h-8 opacity-20" />
                                <p>No se encontraron resultados</p>
                            </div>
                        </TableCell>
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
