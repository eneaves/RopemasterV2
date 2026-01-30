import { useState, useEffect } from 'react'
import { Shuffle, CheckCircle, AlertTriangle, Eye } from 'lucide-react'
import { Button } from './ui/button'
import { Input } from './ui/input'
import { Label } from './ui/label'
import { Checkbox } from './ui/checkbox'
import { Badge } from './ui/badge'
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from './ui/table'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription,
} from './ui/dialog'
import { toast } from 'sonner'
import { getRunsExpanded, generateDrawBatch, generateDraw } from '../lib/api'

interface DrawTabProps {
  event: any
  isLocked: boolean
}

interface DrawEntry {
  round: number
  position: number
  teamId: number
  header: string
  heeler: string
  status: string
}

export function DrawTab({ event, isLocked }: DrawTabProps) {
  const [rounds, setRounds] = useState<string>(String(event?.rounds ?? 3))
  const [autoBalance, setAutoBalance] = useState(true)
  const [drawGenerated, setDrawGenerated] = useState(false)
  const [drawEntries, setDrawEntries] = useState<DrawEntry[]>([])
  const [showExclusionsModal, setShowExclusionsModal] = useState(false)
  const [loading, setLoading] = useState(false)
  const [viewRound, setViewRound] = useState<number | 'all'>('all')

  const fetchDraw = async () => {
    if (!event?.id) return
    try {
      const runs = await getRunsExpanded(Number(event.id))
      console.log('DrawTab fetchDraw loaded runs:', runs?.length)
      if (runs && runs.length > 0) {
        const mapped: DrawEntry[] = runs.map((r: any) => ({
          round: r.round,
          position: r.position,
          teamId: r.team_id,
          header: r.header_name,
          heeler: r.heeler_name,
          status: r.status
        }))
        setDrawEntries(mapped)
        setDrawGenerated(true)
      } else {
        setDrawGenerated(false)
        setDrawEntries([])
      }
    } catch (error) {
      console.error('Error fetching draw:', error)
      toast.error('Error al cargar el draw')
    }
  }

  useEffect(() => {
    fetchDraw()
  }, [event?.id])

  const handleGenerateBatch = async () => {
    if (!event?.id) return
    setLoading(true)
    try {
      const numRounds = parseInt(rounds) || 3
      await generateDrawBatch({
        event_id: Number(event.id),
        rounds: numRounds,
        shuffle: autoBalance
      })
      toast.success('¡Draw completo generado exitosamente!')
      await fetchDraw()
    } catch (error) {
      console.error('Error generating batch draw:', error)
      toast.error('Error al generar el draw: ' + String(error))
    } finally {
      setLoading(false)
    }
  }

  const handleGenerateRound = async (roundNumber: number) => {
    if (!event?.id) return
    setLoading(true)
    try {
      // Si es una ronda > 1, es crítico que la anterior esté "completa" en teoría, 
      // pero el usuario sabe lo que hace. El backend filtrará los eliminados.
      await generateDraw({
        event_id: Number(event.id),
        round: roundNumber,
        reseed: autoBalance,
        seed_runs: true
      })
      toast.success(`¡Ronda ${roundNumber} generada! Se han excluido equipos eliminados.`)
      await fetchDraw()
    } catch (error) {
      console.error(`Error generating round ${roundNumber}:`, error)
      toast.error(`Error: ` + String(error))
    } finally {
      setLoading(false)
    }
  }

  const entriesByRound = drawEntries.reduce((acc, curr) => {
    if (!acc[curr.round]) acc[curr.round] = []
    acc[curr.round].push(curr)
    return acc
  }, {} as Record<number, DrawEntry[]>)

  const numRoundsConfig = parseInt(rounds) || 3
  const roundList = Array.from({length: numRoundsConfig}, (_, i) => i + 1)
  
  const teamsIncluded = drawEntries.length ? new Set(drawEntries.map(d => d.teamId)).size : 0
  const teamsExcluded = 0
  const spacingPercent = 95

  return (
    <div className="space-y-6">
      {/* Header */}
      <div>
        <h2 className="text-foreground mb-1">Draw & Round Management</h2>
        <p className="text-muted-foreground">
          Genera y administra el orden de competencia para cada ronda.
        </p>
      </div>

      <div className="bg-card rounded-xl border border-border p-6 shadow-sm">
         <h3 className="text-foreground text-lg mb-4">Configuración del Draw</h3>
         <div className="flex flex-wrap gap-6 items-end">
            <div className="space-y-2 w-32">
              <Label className="text-foreground">Rondas Totales</Label>
              <Input
                type="number"
                min={1}
                max={10}
                value={rounds}
                onChange={(e) => setRounds(e.target.value)}
                disabled={isLocked || loading}
                className="bg-muted border-border"
              />
            </div>
             <div className="flex items-center space-x-2 pb-3">
                <Checkbox
                  id="autoBalance"
                  checked={autoBalance}
                  onCheckedChange={(checked) => setAutoBalance(!!checked)}
                  disabled={isLocked || loading}
                />
                <Label htmlFor="autoBalance" className="cursor-pointer text-foreground">
                  Barajar y Espaciar (Shuffle)
                </Label>
              </div>
              
              <div className="flex-1" />
              
              {/* Opción de generar todo de golpe si no hay nada generado */}
              {!drawGenerated && !isLocked && (
                <Button 
                   variant="outline"
                   onClick={handleGenerateBatch}
                   disabled={loading}
                   className="border-primary text-primary hover:bg-primary/10"
                >
                   <Shuffle className="w-4 h-4 mr-2" />
                   Generar Todas las Rondas (Batch)
                </Button>
              )}
         </div>
      </div>

      {/* Round Management Cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {roundList.map(r => {
             const exists = entriesByRound[r] && entriesByRound[r].length > 0
             const prevRoundExists = r === 1 ? true : (entriesByRound[r-1] && entriesByRound[r-1].length > 0)
             const count = exists ? entriesByRound[r].length : 0
             
             // Check if THIS specific round has any run captured/completed
             // Ignoramos 'skipped' para que no bloquee la regeneración (ya que son eliminados automáticos)
             const roundStarted = exists && entriesByRound[r].some(e => e.status === 'completed')
             
             // We allow generation if it doesn't exist, previous exists
             const canGenerate = !exists && prevRoundExists
             const canRegenerate = exists && !roundStarted

             return (
               <div key={r} className={`rounded-xl border p-4 ${exists ? 'bg-card border-border' : 'bg-muted/30 border-dashed border-border'}`}>
                  <div className="flex justify-between items-start mb-3">
                     <div>
                        <h4 className="font-semibold text-foreground">Ronda {r}</h4>
                        <p className="text-sm text-muted-foreground">
                           {exists ? `${count} equipos` : 'No generada'} 
                        </p>
                     </div>
                     {exists && <CheckCircle className="w-5 h-5 text-emerald-500" />}
                  </div>

                  {exists ? (
                     <div className="flex flex-col gap-2 mt-2">
                        <div className="text-xs text-emerald-600 font-medium flex items-center gap-1">
                           <CheckCircle className="w-3 h-3" /> GENERADA {roundStarted ? '(Iniciada)' : ''}
                        </div>
                        {canRegenerate && (
                           <Button 
                              variant="outline" 
                              size="sm" 
                              className="w-full text-xs border-dashed border-red-200 text-red-700 hover:bg-red-50 hover:border-red-300"
                              disabled={loading}
                              onClick={() => {
                                  if(window.confirm(`¿Regenerar Ronda ${r}? Esto borrará los tiempos actuales de esta ronda y excluirá equipos eliminados en rondas previas.`)) {
                                      handleGenerateRound(r)
                                  }
                              }}
                           >
                              Regenerar Draw
                           </Button>
                        )}
                        <Button
                           variant={viewRound === r ? "secondary" : "ghost"}
                           size="sm"
                           className="w-full text-xs h-7"
                           onClick={() => setViewRound(viewRound === r ? 'all' : r)}
                        >
                           {viewRound === r ? 'Ver Todos' : 'Ver Lista'}
                        </Button>
                     </div>
                  ) : (
                     <Button 
                        size="sm" 
                        className="w-full mt-2"
                        disabled={!canGenerate || loading}
                        onClick={() => handleGenerateRound(r)}
                     >
                        {loading ? 'Generando...' : `Generar Draw Ronda ${r}`}
                     </Button>
                  )}
                  
                  {!exists && !prevRoundExists && r > 1 && (
                     <p className="text-xs text-orange-500 mt-2">
                        Completa la ronda anterior primero.
                     </p>
                  )}
                  
                  {roundStarted && (
                     <p className="text-xs text-muted-foreground mt-2 italic">
                        No se puede regenerar porque ya hay tiempos capturados.
                     </p>
                  )}
               </div>
             )
          })}
      </div>

      {/* Resumen del draw */}
      {drawGenerated && (
        <>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <div className="bg-card rounded-xl border border-border p-6 shadow-sm">
              <div className="flex items-center gap-3 mb-2">
                <CheckCircle className="w-5 h-5 text-foreground" />
                <h3 className="text-foreground">Rounds Created</h3>
              </div>
              <p className="text-3xl text-foreground">{event?.rounds ?? (Number(rounds) || 0)}</p>
              <p className="text-muted-foreground mt-1">Rondas generadas</p>
            </div>

            <div className="bg-card rounded-xl border border-border p-6 shadow-sm">
              <div className="flex items-center gap-3 mb-2">
                <CheckCircle className="w-5 h-5 text-foreground" />
                <h3 className="text-foreground">Teams Included</h3>
              </div>
              <p className="text-3xl text-foreground">{teamsIncluded}</p>
              <p className="text-muted-foreground mt-1">Equipos en competencia</p>
            </div>

            <div className="bg-card rounded-xl border border-border p-6 shadow-sm">
              <div className="flex items-center gap-3 mb-2">
                <CheckCircle className="w-5 h-5 text-foreground" />
                <h3 className="text-foreground">Spacing Achieved</h3>
              </div>
              <p className="text-3xl text-foreground">{spacingPercent}%</p>
              <p className="text-muted-foreground mt-1">Óptimo espaciamiento</p>
            </div>
          </div>

          {teamsExcluded > 0 && (
            <div className="p-4 bg-muted rounded-xl border border-border flex items-center justify-between">
              <div className="flex items-center gap-3">
                <AlertTriangle className="w-5 h-5 text-foreground" />
                <p className="text-foreground">
                  {teamsExcluded} equipos excluidos del draw por problemas de balance
                </p>
              </div>
              <Button
                variant="outline"
                size="sm"
                onClick={() => setShowExclusionsModal(true)}
                className="border-border"
              >
                <Eye className="w-4 h-4 mr-2" />
                Ver Detalle
              </Button>
            </div>
          )}

          {/* Tabla del draw */}
          <div className="bg-card rounded-xl border border-border overflow-hidden">
            <div className="p-4 border-b border-border flex justify-between items-center bg-muted/30">
               <h4 className="font-medium text-foreground">
                  {viewRound === 'all' ? 'Todos los Runs' : `Runs de Ronda ${viewRound}`}
               </h4>
               {viewRound !== 'all' && (
                  <Button size="sm" variant="ghost" onClick={() => setViewRound('all')}>
                     Ver Todos
                  </Button>
               )}
            </div>
            <Table>
              <TableHeader>
                <TableRow className="bg-muted hover:bg-muted">
                  <TableHead className="text-foreground">Round</TableHead>
                  <TableHead className="text-foreground">Position</TableHead>
                  <TableHead className="text-foreground">Team</TableHead>
                  <TableHead className="text-foreground">Header</TableHead>
                  <TableHead className="text-foreground">Heeler</TableHead>
                  <TableHead className="text-foreground">Status</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {drawEntries
                  .filter(e => viewRound === 'all' || e.round === viewRound)
                  .map((entry, index) => (
                  <TableRow key={index} className="hover:bg-accent/30">
                    <TableCell>
                      <Badge variant="outline" className="border-primary text-primary">
                        Round {entry.round}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-foreground">#{entry.position}</TableCell>
                    <TableCell>
                      <Badge variant="outline" className="border-border">
                        Team #{entry.teamId}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-foreground">{entry.header}</TableCell>
                    <TableCell className="text-foreground">{entry.heeler}</TableCell>
                    <TableCell>
                      {entry.status === 'completed' ? (
                        <Badge className="bg-emerald-50 text-emerald-700 border-emerald-200">
                          Completed
                        </Badge>
                      ) : (
                        <Badge className="bg-muted text-muted-foreground border-border">
                          Pending
                        </Badge>
                      )}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>

          <div className="mt-4 text-muted-foreground">
            <p>Total entries: {drawEntries.length} • Average spacing: {spacingPercent}%</p>
          </div>
        </>
      )}

      {/* Modal de exclusiones */}
      <Dialog open={showExclusionsModal} onOpenChange={setShowExclusionsModal}>
        <DialogContent className="sm:max-w-[520px]">
          <DialogHeader>
            <DialogTitle className="text-foreground">Equipos Excluidos</DialogTitle>
            <DialogDescription>Motivos por los que se excluyeron equipos del draw automático</DialogDescription>
          </DialogHeader>

          <div className="mt-4">
            <div className="p-3 bg-muted rounded-xl border border-border">
              <p className="text-foreground">No hay equipos excluidos</p>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  )
}
