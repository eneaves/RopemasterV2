import { useState, useEffect, useCallback } from 'react'
import { DollarSign, Percent, RefreshCw, Wand2, Trash2 } from 'lucide-react'
import { Button } from './ui/button'
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from './ui/table'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from './ui/select'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from './ui/alert-dialog'
import { toast } from 'sonner'
import { listPayoffRules, createPayoffRule, deletePayoffRule, getPayoutBreakdown, getStandings } from '../lib/api'

interface PayoffsTabProps {
  event: any
  onFinalize?: () => void
  isFinalized?: boolean
}

interface PayoffRule {
  id: number
  position: number
  percentage: number
}

interface PayoutBreakdown {
  total_pot: number
  deductions: number
  net_pot: number
  payouts: Array<{ place: number; percentage: number; amount: number }>
}

interface Standing {
  rank: number
  teamId: number
  header: string
  heeler: string
  totalTime: number
  completedRuns: number
}

const PRESETS = [
  { label: '1 Lugar (100%)', rules: [{ position: 1, percentage: 100 }] },
  { label: '2 Lugares (60/40)', rules: [{ position: 1, percentage: 60 }, { position: 2, percentage: 40 }] },
  { label: '3 Lugares (50/30/20)', rules: [{ position: 1, percentage: 50 }, { position: 2, percentage: 30 }, { position: 3, percentage: 20 }] },
  { label: '4 Lugares (40/30/20/10)', rules: [{ position: 1, percentage: 40 }, { position: 2, percentage: 30 }, { position: 3, percentage: 20 }, { position: 4, percentage: 10 }] },
]

export function PayoffsTab({ event }: PayoffsTabProps) {
  const [rules, setRules] = useState<PayoffRule[]>([])
  const [breakdown, setBreakdown] = useState<PayoutBreakdown | null>(null)
  const [standings, setStandings] = useState<Standing[]>([])
  const [selectedPreset, setSelectedPreset] = useState<string>('')
  const [loading, setLoading] = useState(false)

  const fetchData = useCallback(async () => {
    if (!event?.id) return
    setLoading(true)
    try {
      // 1. Fetch rules
      try {
        const rulesData = await listPayoffRules(Number(event.id))
        const uiRules = rulesData.map((r: any) => ({ ...r, percentage: r.percentage * 100 }))
        setRules(uiRules)
      } catch (e) {
        console.error('Error fetching rules:', e)
        toast.error('Error al cargar reglas')
        return
      }

      // 2. Fetch breakdown & standings
      try {
        const breakdownData = await getPayoutBreakdown(Number(event.id))
        
        if (breakdownData) {
          breakdownData.payouts = breakdownData.payouts.map((p: any) => ({
            ...p,
            percentage: p.percentage * 100
          }))
        }
        setBreakdown(breakdownData)

        try {
           const standingsData = await getStandings(Number(event.id))
           const mappedStandings: Standing[] = standingsData.map((s: any) => ({
              rank: s.rank,
              teamId: s.team_id,
              header: s.header_name || 'Desconocido',
              heeler: s.heeler_name || 'Desconocido',
              totalTime: s.total_time,
              completedRuns: s.completed_runs
            }))
            setStandings(mappedStandings)
        } catch (e) {
            console.warn('Error fetching standings (non-critical):', e)
            setStandings([])
        }

      } catch (e) {
        console.warn('Error fetching breakdown/standings:', e)
        // If breakdown fails, we can't show projections, but basic UI should remain
        setBreakdown(null)
      }
    } catch (error) {
      console.error('Unexpected error:', error)
    } finally {
      setLoading(false)
    }
  }, [event?.id])

  useEffect(() => {
    fetchData()
  }, [fetchData])

  const executeApplyPreset = async () => {
    if (!selectedPreset) return
    const index = parseInt(selectedPreset)
    const preset = PRESETS[index]
    if (!preset) return

    setLoading(true)
    try {
      // Delete existing
      await Promise.all(rules.map(r => deletePayoffRule(r.id)))
      // Create new
      await Promise.all(preset.rules.map(r => createPayoffRule({
        event_id: Number(event.id),
        position: r.position,
        percentage: r.percentage / 100.0
      })))
      
      toast.success(`Preset aplicado: ${preset.label}`)
      setSelectedPreset('')
      fetchData()
    } catch (error) {
      console.error(error)
      toast.error('Error al aplicar preset')
      setLoading(false)
    }
  }
  
  const handleDeleteRule = async (id: number) => {
    try {
      await deletePayoffRule(id)
      toast.success('Regla eliminada')
      fetchData()
    } catch (error) {
      console.error(error)
      toast.error('Error al eliminar regla')
    }
  }

  const formatCurrency = (val: number) => {
    return new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD' }).format(val)
  }

  return (
    <div className="space-y-6 h-full flex flex-col">
      <div className="flex justify-between items-center">
        <div>
          <h2 className="text-foreground mb-1">Gestión de Payoffs</h2>
          <p className="text-muted-foreground">Configura la distribución de premios usando presets.</p>
        </div>
        <Button variant="outline" onClick={fetchData} disabled={loading} className="border-border">
          <RefreshCw className={`w-4 h-4 mr-2 ${loading ? 'animate-spin' : ''}`} />
          Actualizar
        </Button>
      </div>

      {/* KPI Cards */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <div className="bg-card border border-border rounded-xl p-4 shadow-sm">
          <div className="flex items-center gap-2 mb-2 text-muted-foreground">
            <DollarSign className="w-4 h-4" />
            <span className="text-sm font-medium">Total Pot (Gross)</span>
          </div>
          <div className="text-2xl font-bold text-foreground">
            {breakdown ? formatCurrency(breakdown.total_pot) : '—'}
          </div>
        </div>

        <div className="bg-card border border-border rounded-xl p-4 shadow-sm">
          <div className="flex items-center gap-2 mb-2 text-muted-foreground">
            <Percent className="w-4 h-4" />
            <span className="text-sm font-medium">Deducciones</span>
          </div>
          <div className="text-2xl font-bold text-red-600">
            {breakdown ? formatCurrency(breakdown.deductions) : '—'}
          </div>
        </div>

        <div className="bg-card border border-border rounded-xl p-4 shadow-sm bg-emerald-50/50 dark:bg-emerald-950/20">
          <div className="flex items-center gap-2 mb-2 text-emerald-700 dark:text-emerald-400">
            <DollarSign className="w-4 h-4" />
            <span className="text-sm font-medium">Net Pot (A Repartir)</span>
          </div>
          <div className="text-2xl font-bold text-emerald-700 dark:text-emerald-400">
            {breakdown ? formatCurrency(breakdown.net_pot) : '—'}
          </div>
        </div>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6 flex-1 min-h-0">
        {/* Left: Rules Configuration (PRESETS ONLY) */}
        <div className="bg-card border border-border rounded-xl flex flex-col overflow-hidden shadow-sm">
          <div className="p-4 border-b border-border bg-muted/30">
            <h3 className="font-medium text-foreground mb-1">Configuración de Reglas</h3>
            <p className="text-xs text-muted-foreground">Selecciona un esquema de distribución predefinido.</p>
          </div>
          
          <div className="p-6 flex flex-col gap-4 bg-card">
              <div className="flex gap-3">
                 <Select value={selectedPreset} onValueChange={setSelectedPreset}>
                    <SelectTrigger className="flex-1">
                    <SelectValue placeholder="Seleccionar Preset de Distribución..." />
                    </SelectTrigger>
                    <SelectContent>
                    {PRESETS.map((preset, idx) => (
                        <SelectItem key={idx} value={idx.toString()}>
                        {preset.label}
                        </SelectItem>
                    ))}
                    </SelectContent>
                </Select>
                
                <AlertDialog>
                    <AlertDialogTrigger asChild>
                    <Button 
                        disabled={!selectedPreset || loading}
                        className="bg-primary text-primary-foreground min-w-[100px]"
                    >
                        <Wand2 className="w-4 h-4 mr-2" />
                        Aplicar
                    </Button>
                    </AlertDialogTrigger>
                    <AlertDialogContent>
                    <AlertDialogHeader>
                        <AlertDialogTitle>¿Aplicar Preset?</AlertDialogTitle>
                        <AlertDialogDescription>
                        Se aplicará la distribución "{selectedPreset && PRESETS[parseInt(selectedPreset)]?.label}". 
                        
                        Atención: Esto borrará cualquier regla personalizada existente.
                        </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                        <AlertDialogCancel>Cancelar</AlertDialogCancel>
                        <AlertDialogAction onClick={executeApplyPreset}>Confirmar</AlertDialogAction>
                    </AlertDialogFooter>
                    </AlertDialogContent>
                </AlertDialog>
              </div>
          </div>

          <div className="flex-1 overflow-auto border-t border-border">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-20">Lugar</TableHead>
                  <TableHead>Porcentaje Asignado</TableHead>
                  <TableHead className="text-right">Acción</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {rules.length === 0 ? (
                  <TableRow>
                    <TableCell colSpan={3} className="text-center py-12 text-muted-foreground bg-muted/5">
                      <div className="flex flex-col items-center gap-2 opacity-50">
                          <Percent className="w-8 h-8" />
                          <p>No hay reglas activas. Selecciona un preset.</p>
                      </div>
                    </TableCell>
                  </TableRow>
                ) : (
                  rules.map((rule) => (
                    <TableRow key={rule.id}>
                      <TableCell className="font-medium bg-muted/20">#{rule.position}</TableCell>
                      <TableCell className="font-mono text-lg">{rule.percentage}%</TableCell>
                      <TableCell className="text-right">
                         <div className="flex justify-end">
                            <Button 
                              variant="ghost" 
                              size="icon"
                              onClick={() => handleDeleteRule(rule.id)}
                              className="w-8 h-8 text-muted-foreground hover:text-destructive hover:bg-destructive/10"
                            >
                              <Trash2 className="w-4 h-4" />
                            </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
          </div>
          <div className="p-3 bg-muted/30 border-t border-border text-xs text-muted-foreground text-center font-medium">
            Total asignado: {rules.reduce((acc, r) => acc + r.percentage, 0).toFixed(2)}%
          </div>
        </div>

        {/* Right: Projected Payouts */}
        <div className="bg-card border border-border rounded-xl flex flex-col overflow-hidden shadow-sm">
          <div className="p-4 border-b border-border bg-muted/30">
            <h3 className="font-medium text-foreground mb-1">Proyección de Pagos</h3>
             <p className="text-xs text-muted-foreground">Basado en los standings actuales (Top {breakdown?.payouts.length || 0})</p>
          </div>
          
          <div className="flex-1 overflow-auto">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-16">Lugar</TableHead>
                  <TableHead>Equipo Ganador</TableHead>
                  <TableHead className="text-right">Monto Total</TableHead>
                  <TableHead className="text-right">Por Persona</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {!breakdown || breakdown.payouts.length === 0 ? (
                  <TableRow>
                    <TableCell colSpan={4} className="text-center py-12 text-muted-foreground bg-muted/5">
                      <div className="flex flex-col items-center gap-2 opacity-50">
                          <DollarSign className="w-8 h-8" />
                          <p>Configura reglas o espera resultados para ver proyección.</p>
                      </div>
                    </TableCell>
                  </TableRow>
                ) : (
                  breakdown.payouts.map((p) => {
                    const team = standings.find(s => s.rank === p.place)
                    return (
                      <TableRow key={p.place}>
                        <TableCell className="font-medium">
                          <div className={`flex items-center justify-center w-8 h-8 rounded-full text-sm font-bold border 
                             ${p.place === 1 ? 'bg-amber-100 text-amber-700 border-amber-200' : 
                               p.place === 2 ? 'bg-gray-100 text-gray-700 border-gray-200' :
                               p.place === 3 ? 'bg-orange-100 text-orange-700 border-orange-200' : 'bg-muted text-muted-foreground border-border'}`}>
                            {p.place}
                          </div>
                        </TableCell>
                        <TableCell>
                          {team ? (
                            <div className="flex flex-col">
                              <span className="font-semibold text-foreground">{team.header} <span className="text-muted-foreground font-normal">&</span> {team.heeler}</span>
                              <span className="text-xs text-muted-foreground font-mono mt-0.5">
                                Team #{team.teamId} • {team.totalTime?.toFixed(2)}s
                              </span>
                            </div>
                          ) : (
                            <div className="flex items-center gap-2 text-muted-foreground italic">
                               <span className="w-2 h-2 rounded-full bg-muted-foreground/30" />
                               Pendiente / Vacante
                            </div>
                          )}
                        </TableCell>
                        <TableCell className="text-right font-bold text-lg text-emerald-600">
                          {formatCurrency(p.amount)}
                        </TableCell>
                        <TableCell className="text-right text-muted-foreground font-medium">
                          {formatCurrency(p.amount / 2)}
                        </TableCell>
                      </TableRow>
                    )
                  })
                )}
              </TableBody>
            </Table>
          </div>
        </div>
      </div>
    </div>
  )
}
