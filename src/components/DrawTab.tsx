import { useState, useEffect } from 'react'
import { Shuffle, CheckCircle } from 'lucide-react'
import { Button } from './ui/button'
import { Input } from './ui/input'
import { Label } from './ui/label'
import { Badge } from './ui/badge'
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from './ui/table'
import { toast } from 'sonner'
import { getRunsExpanded, generateDrawBatch } from '../lib/api'

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
  const [drawGenerated, setDrawGenerated] = useState(false)
  const [drawEntries, setDrawEntries] = useState<DrawEntry[]>([])
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
        shuffle: true,
      })
      if (numRounds > 1) {
        toast.success(`¡Rondas 1 a ${numRounds - 1} generadas! La ronda ${numRounds} (final) debe generarse después de completar las rondas anteriores.`)
      } else {
        toast.success('¡Draw generado exitosamente!')
      }
      await fetchDraw()
    } catch (error) {
      console.error('Error generating batch draw:', error)
      toast.error('Error al generar el draw: ' + String(error))
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
  const generatedRounds = Object.keys(entriesByRound).length
  
  const teamsIncluded = drawEntries.length ? new Set(drawEntries.map(d => d.teamId)).size : 0
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
            <div className="flex-1" />

            {!drawGenerated && !isLocked && (
              <Button
                variant="outline"
                onClick={handleGenerateBatch}
                disabled={loading}
                className="border-primary text-primary hover:bg-primary/10"
                title="Genera las rondas iniciales. La ronda final se generará cuando se completen los runs anteriores."
              >
                <Shuffle className="w-4 h-4 mr-2" />
                Generar Rondas
              </Button>
            )}
         </div>
      </div>

      {/* Round Management Cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {roundList.map(r => {
             const exists = entriesByRound[r] && entriesByRound[r].length > 0
             const count = exists ? entriesByRound[r].length : 0
             const isFinalRound = r === numRoundsConfig
             const roundStarted = exists && entriesByRound[r].some(e => e.status === 'completed')

             return (
               <div key={r} className={`rounded-xl border p-4 ${exists ? 'bg-card border-border' : 'bg-muted/30 border-dashed border-border'}`}>
                  <div className="flex justify-between items-start mb-3">
                     <div>
                        <h4 className="font-semibold text-foreground">
                           Ronda {r} {isFinalRound && <Badge variant="secondary" className="ml-2 text-xs">Final</Badge>}
                        </h4>
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
                        <p className="text-xs text-muted-foreground">
                          {isFinalRound
                            ? 'Ordenada por tiempos acumulados.'
                            : 'Incluida en la generación inicial.'}
                        </p>
                  </div>
                  ) : isFinalRound ? (
                    <p className="text-xs text-orange-500 mt-2">
                      Esta ronda se generará cuando haya completado todos los runs anteriores.
                    </p>
                  ) : (
                    <p className="text-xs text-muted-foreground mt-2">
                      Se generará con el botón principal.
                    </p>
                  )}

                  {roundStarted && (
                     <p className="text-xs text-muted-foreground mt-2 italic">
                        Ya hay tiempos capturados en esta ronda.
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
              <p className="text-3xl text-foreground">{generatedRounds}</p>
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
                      ) : entry.status === 'skipped' ? (
                        <Badge className="bg-amber-50 text-amber-700 border-amber-200">
                          Skipped
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
    </div>
  )
}
