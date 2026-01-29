import { useState, useEffect } from 'react'
import {
  RotateCcw,
  Download,
  Lock,
  X,
  ChevronLeft,
  ChevronRight,
  Save,
  Play,
  Pause,
  Clock,
  Users,
  Activity,
  ArrowLeft,
} from 'lucide-react'
import { Button } from './ui/button'
import { Input } from './ui/input'
import { Label } from './ui/label'
import { Badge } from './ui/badge'
import { Checkbox } from './ui/checkbox'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from './ui/select'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from './ui/table'
import { toast } from 'sonner'
import { getRunsExpanded, saveRun, exportEvent, updateEventStatus } from '../lib/api'
import { save } from '@tauri-apps/plugin-dialog'

interface Team {
  id: string
  teamId: number
  header: string
  heeler: string
  time: number | null
  penalty: number
  total: number | null
  status: 'pending' | 'completed' | 'nt' | 'dq'
  position?: number
}

interface EventCaptureViewProps {
  event: any
  series: any
  onBack: () => void
}

export function EventCaptureView({ event, series, onBack }: EventCaptureViewProps) {
  const [teams, setTeams] = useState<Team[]>([])
  const [currentRound, setCurrentRound] = useState(1)
  const [currentTeamIndex, setCurrentTeamIndex] = useState<number | null>(null)
  const [isLocked, setIsLocked] = useState(event.status === 'locked')
  const [timerRunning, setTimerRunning] = useState(false)
  const [timerValue, setTimerValue] = useState(0)
  const [penalty, setPenalty] = useState('0')
  const [isNT, setIsNT] = useState(false)
  const [isDQ, setIsDQ] = useState(false)

  const currentTeam = currentTeamIndex !== null ? teams[currentTeamIndex] : null
  const totalRounds = event.rounds
  const totalTeams = teams.length
  const completedRuns =
    teams.filter((t) => t.status === 'completed' || t.status === 'nt' || t.status === 'dq').length

  // Fetch runs
  const fetchRuns = async () => {
    try {
      const data = await getRunsExpanded(parseInt(event.id), currentRound)
      const mapped = data.map((r: any) => {
        let status: Team['status'] = 'pending'
        if (r.dq) status = 'dq'
        else if (r.no_time) status = 'nt'
        else if (r.status === 'completed') status = 'completed'
        
        return {
          id: r.id.toString(),
          teamId: r.team_id,
          header: r.header_name,
          heeler: r.heeler_name,
          time: r.time_sec,
          penalty: r.penalty,
          total: r.total_sec,
          status: status,
          position: r.position
        }
      })
      setTeams(mapped)
    } catch (e) {
      console.error(e)
      toast.error('Error al cargar runs')
    }
  }

  useEffect(() => {
    fetchRuns()
  }, [event.id, currentRound])

  // Timer
  useEffect(() => {
    let interval: ReturnType<typeof setInterval>
    if (timerRunning) {
      interval = setInterval(() => setTimerValue((prev) => prev + 10), 10)
    }
    return () => clearInterval(interval)
  }, [timerRunning])

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyPress = (e: KeyboardEvent) => {
      if (!currentTeam) return
      if (e.target instanceof HTMLInputElement) return

      switch (e.key.toLowerCase()) {
        case ' ':
          e.preventDefault()
          setTimerRunning((prev) => !prev)
          break
        case 'enter':
          e.preventDefault()
          handleSaveRun()
          break
        case 'n':
          e.preventDefault()
          handleNextTeam()
          break
        case 'p':
          e.preventDefault()
          handlePreviousTeam()
          break
        case 'r':
          e.preventDefault()
          handleReset()
          break
        case 'escape':
          e.preventDefault()
          handleCloseCapture()
          break
      }
    }

    window.addEventListener('keydown', handleKeyPress)
    return () => window.removeEventListener('keydown', handleKeyPress)
  }, [currentTeam, timerRunning, timerValue, penalty, isNT, isDQ])

  const formatTime = (ms: number) => {
    const totalSeconds = ms / 1000
    const minutes = Math.floor(totalSeconds / 60)
    const seconds = Math.floor(totalSeconds % 60)
    const milliseconds = Math.floor((ms % 1000) / 10)
    return `${minutes}:${seconds.toString().padStart(2, '0')}.${milliseconds
      .toString()
      .padStart(2, '0')}`
  }

  const formatDisplayTime = (seconds: number) => seconds.toFixed(2)

  const handleStartCapture = (index: number) => {
    setCurrentTeamIndex(index)
    handleReset()
  }

  const handleCloseCapture = () => {
    setCurrentTeamIndex(null)
    handleReset()
  }

  const handleReset = () => {
    setTimerRunning(false)
    setTimerValue(0)
    setPenalty('0')
    setIsNT(false)
    setIsDQ(false)
  }

  const handleSaveRun = async () => {
    if (currentTeamIndex === null) return

    const timeInSeconds = timerValue / 1000
    const penaltyValue = parseFloat(penalty) || 0
    const team = teams[currentTeamIndex]

    // Optimistic update
    const updatedTeams = [...teams]
    const updatedTeam = { ...team }
    
    let noTime = false
    let dq = false
    let timeSec: number | null = timeInSeconds

    if (isDQ) {
      updatedTeam.status = 'dq'
      updatedTeam.time = null
      updatedTeam.total = null
      dq = true
      timeSec = null
    } else if (isNT) {
      updatedTeam.status = 'nt'
      updatedTeam.time = null
      updatedTeam.total = null
      noTime = true
      timeSec = null
    } else {
      updatedTeam.status = 'completed'
      updatedTeam.time = timeInSeconds
      updatedTeam.penalty = penaltyValue
      updatedTeam.total = timeInSeconds + penaltyValue
    }
    
    updatedTeams[currentTeamIndex] = updatedTeam
    setTeams(updatedTeams)

    try {
      await saveRun({
        event_id: parseInt(event.id),
        team_id: team.teamId,
        round: currentRound,
        position: team.position || (currentTeamIndex + 1),
        time_sec: timeSec,
        penalty: penaltyValue,
        no_time: noTime,
        dq: dq,
        captured_by: null
      })
      
      if (!isLocked && currentTeamIndex === 0) {
        setIsLocked(true)
        updateEventStatus(parseInt(event.id), 'locked').catch(console.error)
        toast.success('Run guardado! Evento bloqueado automáticamente.')
      } else {
        toast.success('Run guardado correctamente.')
      }
      handleNextTeam()
    } catch (e) {
      console.error(e)
      toast.error('Error al guardar run')
      fetchRuns() // Revert on error
    }
  }

  const handleNextTeam = () => {
    if (currentTeamIndex !== null && currentTeamIndex < teams.length - 1) {
      setCurrentTeamIndex(currentTeamIndex + 1)
      handleReset()
    }
  }

  const handlePreviousTeam = () => {
    if (currentTeamIndex !== null && currentTeamIndex > 0) {
      setCurrentTeamIndex(currentTeamIndex - 1)
      handleReset()
    }
  }

  const handleExport = async () => {
    try {
      const filePath = await save({
        filters: [{
          name: 'Excel',
          extensions: ['xlsx']
        }]
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
        toast.success('Evento exportado exitosamente')
      }
    } catch (e) {
      console.error(e)
      toast.error('Error al exportar')
    }
  }

  const handleResetRound = () => {
    toast.info('Reiniciar ronda - Próximamente')
  }

  const sortedResults = [...teams]
    .filter((t) => t.status !== 'pending')
    .sort((a, b) => {
      if (a.status === 'dq') return 1
      if (b.status === 'dq') return -1
      if (a.status === 'nt') return 1
      if (b.status === 'nt') return -1
      return (a.total || 0) - (b.total || 0)
    })

  const averageTime =
    teams.filter((t) => t.total !== null).reduce((sum, t) => sum + (t.total || 0), 0) /
      (teams.filter((t) => t.total !== null).length || 1)

  const [resultsExpanded, setResultsExpanded] = useState(false)

  const getStatusBadge = () => {
    if (isLocked) {
      return <Badge className="bg-accent text-primary border-accent"><Lock className="mr-1 h-3 w-3" /> Locked</Badge>
    }
    return <Badge className="bg-emerald-50 text-emerald-700 border-emerald-200">🟢 Active</Badge>
  }

  return (
    <div className="flex-1 bg-background h-screen overflow-hidden flex flex-col">
      {/* Header */}
      <div className="bg-card border-b border-border px-6 py-4">
        <div className="flex justify-between items-start mb-3">
          <div className="flex items-center gap-4">
            <Button onClick={onBack} variant="ghost" size="icon" className="hover:bg-accent rounded-xl">
              <ArrowLeft className="h-5 w-5" />
            </Button>
            <div>
              <h1 className="text-foreground mb-1">Evento: {event.name}</h1>
              <p className="text-muted-foreground">
                Serie: {series.name} · Rondas: {totalRounds} · Equipos: {totalTeams}
              </p>
            </div>
          </div>
          <div className="flex items-center gap-3">
            {getStatusBadge()}
            <Button onClick={handleResetRound} variant="outline" className="border-border text-foreground hover:bg-background rounded-xl h-11">
              <RotateCcw className="h-4 w-4 mr-2" />
              Reiniciar ronda
            </Button>
            <Button onClick={handleExport} className="bg-primary hover:opacity-90 text-primary-foreground rounded-xl shadow-sm h-11">
              <Download className="h-4 w-4 mr-2" />
              Exportar XLSX
            </Button>
          </div>
        </div>

        <div className="flex items-center gap-4">
          <div className="flex items-center gap-2">
            <Label className="text-muted-foreground">Ronda actual:</Label>
            <Select value={currentRound.toString()} onValueChange={(v) => setCurrentRound(parseInt(v))}>
              <SelectTrigger className="w-[120px] bg-card border-border rounded-xl h-11">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {Array.from({ length: totalRounds }, (_, i) => i + 1).map((round) => (
                  <SelectItem key={round} value={round.toString()}>
                    Ronda {round}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <span className="text-muted-foreground">/ {totalRounds}</span>
          </div>
        </div>
      </div>

      {/* Main */}
      <div className="flex-1 overflow-hidden flex">
        {/* Left: Teams list */}
        <div className="w-96 bg-card border-r border-border flex flex-col">
          <div className="p-4 border-b border-border">
            <h2 className="text-foreground">Rondas & Captura rápida</h2>
          </div>

          <div className="flex-1 overflow-y-auto">
            <Table>
              <TableHeader className="sticky top-0 bg-muted z-10">
                <TableRow className="hover:bg-muted">
                  <TableHead className="text-foreground w-12">#</TableHead>
                  <TableHead className="text-foreground">Equipo</TableHead>
                  <TableHead className="text-foreground w-20">Estado</TableHead>
                  <TableHead className="text-foreground w-24 text-right">Acción</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {teams.map((team, index) => (
                  <TableRow
                    key={team.id}
                    className={`hover:bg-muted/60 cursor-pointer ${currentTeamIndex === index ? 'bg-accent/40' : ''}`}
                    onClick={() => handleStartCapture(index)}
                  >
                    <TableCell>
                      <Badge variant="outline" className="border-border">
                        {index + 1}
                      </Badge>
                    </TableCell>
                    <TableCell>
                      <div>
                        <div className="text-foreground">{team.header}</div>
                        <div className="text-muted-foreground">{team.heeler}</div>
                      </div>
                    </TableCell>
                    <TableCell>
                      {team.status === 'completed' && (
                        <Badge className="bg-emerald-50 text-emerald-700 border-emerald-200">✓</Badge>
                      )}
                      {team.status === 'nt' && (
                        <Badge className="bg-red-50 text-red-700 border-red-200">NT</Badge>
                      )}
                      {team.status === 'dq' && (
                        <Badge className="bg-red-50 text-red-700 border-red-200">DQ</Badge>
                      )}
                      {team.status === 'pending' && <span className="text-muted-foreground">—</span>}
                    </TableCell>
                    <TableCell className="text-right">
                      <Button
                        size="sm"
                        variant={currentTeamIndex === index ? 'default' : 'outline'}
                        className={currentTeamIndex === index ? 'bg-primary text-primary-foreground' : 'border-border'}
                      >
                        Capturar
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>

          <div className="p-4 border-t border-border bg-muted/50">
            <p className="text-muted-foreground">
              <strong>Atajos:</strong> Espacio → start/stop · Enter → guardar · N → siguiente · P → anterior · Esc → cerrar
            </p>
          </div>
        </div>

        {/* Right: Capture + Results */}
        <div className="flex-1 overflow-y-auto p-6 flex flex-col gap-6">
          {/* Capture Panel */}
          {currentTeam ? (
            <div className="bg-card rounded-xl border border-border shadow-sm p-8">
              {/* Header */}
              <div className="flex justify-between items-start mb-6">
                <div>
                  <div className="flex items-center gap-3 mb-2">
                    <Badge className="bg-primary text-primary-foreground px-3 py-1 rounded-md">
                      Ronda {currentRound} · Run #{(currentTeamIndex || 0) + 1}
                    </Badge>
                  </div>
                  <h2 className="text-foreground">
                    Equipo actual: {currentTeam.header} & {currentTeam.heeler}
                  </h2>
                </div>
                <Button onClick={handleCloseCapture} variant="ghost" size="icon" className="hover:bg-muted rounded-xl">
                  <X className="h-5 w-5" />
                </Button>
              </div>

              {/* Timer */}
              <div className="mb-8">
                <div className="bg-foreground rounded-xl p-8 text-center mb-4">
                  <div className="flex items-center justify-center gap-3 mb-3">
                    <Clock className="h-6 w-6 text-primary" />
                    <span className="text-background/80 tracking-wider">CRONÓMETRO</span>
                  </div>
                  <div className="text-7xl text-primary font-mono tabular-nums">
                    {formatTime(timerValue)}
                  </div>
                </div>

                <div className="flex gap-3">
                  <Button
                    onClick={() => setTimerRunning(!timerRunning)}
                    className={`flex-1 h-14 text-lg text-white rounded-xl ${
                      timerRunning ? 'bg-foreground hover:opacity-90' : 'bg-emerald-600 hover:bg-emerald-700'
                    }`}
                  >
                    {timerRunning ? (
                      <>
                        <Pause className="h-5 w-5 mr-2" />
                        Pausar (Espacio)
                      </>
                    ) : (
                      <>
                        <Play className="h-5 w-5 mr-2" />
                        Iniciar (Espacio)
                      </>
                    )}
                  </Button>
                  <Button onClick={handleReset} variant="outline" className="h-14 border-border text-foreground hover:bg-background rounded-xl px-6">
                    <RotateCcw className="h-5 w-5 mr-2" />
                    Reset (R)
                  </Button>
                </div>
              </div>

              {/* Inputs */}
              <div className="grid grid-cols-3 gap-4 mb-6">
                <div className="space-y-2">
                  <Label htmlFor="penalty" className="text-foreground">
                    Penal (s)
                  </Label>
                  <Input
                    id="penalty"
                    type="number"
                    step="1"
                    value={penalty}
                    onChange={(e) => setPenalty(e.target.value)}
                    className="bg-muted border-border rounded-xl h-12 text-lg"
                  />
                </div>

                <div className="flex items-end">
                  <div className="flex items-center space-x-2 h-12">
                    <Checkbox
                      id="nt"
                      checked={isNT}
                      onCheckedChange={(checked) => {
                        setIsNT(checked as boolean)
                        if (checked) setIsDQ(false)
                      }}
                    />
                    <Label htmlFor="nt" className="text-foreground cursor-pointer text-lg">
                      NT (No Time)
                    </Label>
                  </div>
                </div>

                <div className="flex items-end">
                  <div className="flex items-center space-x-2 h-12">
                    <Checkbox
                      id="dq"
                      checked={isDQ}
                      onCheckedChange={(checked) => {
                        setIsDQ(checked as boolean)
                        if (checked) setIsNT(false)
                      }}
                    />
                    <Label htmlFor="dq" className="text-foreground cursor-pointer text-lg">
                      DQ (Descalificado)
                    </Label>
                  </div>
                </div>
              </div>

              {/* Action Buttons */}
              <div className="flex gap-3">
                <Button
                  onClick={handlePreviousTeam}
                  disabled={currentTeamIndex === 0}
                  variant="outline"
                  className="border-border text-foreground hover:bg-background rounded-xl h-12 px-6"
                >
                  <ChevronLeft className="h-5 w-5 mr-2" />
                  Anterior (P)
                </Button>
                <Button className="flex-1 bg-emerald-500 hover:bg-emerald-600 text-white rounded-xl h-12 text-lg shadow-sm" onClick={handleSaveRun}>
                  <Save className="h-5 w-5 mr-2" />
                  Guardar (Enter)
                </Button>
                <Button
                  onClick={handleNextTeam}
                  disabled={currentTeamIndex === teams.length - 1}
                  variant="outline"
                  className="border-border text-foreground hover:bg-background rounded-xl h-12 px-6"
                >
                  Siguiente (N)
                  <ChevronRight className="h-5 w-5 ml-2" />
                </Button>
              </div>
            </div>
          ) : (
            <div className="bg-card rounded-xl border border-border p-12 text-center shadow-sm">
              <Clock className="h-16 w-16 text-muted-foreground mx-auto mb-4" />
              <h3 className="text-foreground mb-2">Selecciona un equipo para capturar</h3>
              <p className="text-muted-foreground">Haz clic en "Capturar" en la lista de equipos para comenzar</p>
            </div>
          )}

          {/* Stats */}
          <div className="grid grid-cols-3 gap-4">
            <div className="bg-card rounded-xl border border-border p-4 shadow-sm">
              <div className="flex items-center gap-3 mb-2">
                <Activity className="h-5 w-5 text-blue-600" />
                <p className="text-muted-foreground">Runs capturados</p>
              </div>
              <p className="text-2xl text-foreground">
                {completedRuns} / {totalTeams}
              </p>
            </div>

            <div className="bg-card rounded-xl border border-border p-4 shadow-sm">
              <div className="flex items-center gap-3 mb-2">
                <Users className="h-5 w-5 text-emerald-600" />
                <p className="text-muted-foreground">Ronda</p>
              </div>
              <p className="text-2xl text-foreground">
                {currentRound} / {totalRounds}
              </p>
            </div>

            <div className="bg-card rounded-xl border border-border p-4 shadow-sm">
              <div className="flex items-center gap-3 mb-2">
                <Clock className="h-5 w-5 text-violet-600" />
                <p className="text-muted-foreground">Promedio general</p>
              </div>
              <p className="text-2xl text-foreground">
                {averageTime > 0 ? `${averageTime.toFixed(2)}s` : '—'}
              </p>
            </div>
          </div>

          {/* Results */}
          <div className="bg-card rounded-xl border border-border overflow-hidden flex flex-col">
            <div className="p-4 bg-muted border-b border-border flex justify-between items-center">
              <h2 className="text-foreground">Resultados Parciales (Ronda actual)</h2>
              <div className="flex items-center gap-3">
                <Button variant="outline" size="sm" onClick={() => setResultsExpanded((s) => !s)}>
                  {resultsExpanded ? 'Reducir' : 'Expandir'}
                </Button>
                {isLocked && (
                  <Badge className="bg-accent text-primary border-accent">
                    <Lock className="h-3 w-3 mr-1" />
                    Locked
                  </Badge>
                )}
              </div>
            </div>

            <div className={`p-0 ${resultsExpanded ? 'max-h-[70vh]' : 'max-h-64'} overflow-auto`}> 
              <Table>
                <TableHeader>
                  <TableRow className="bg-muted hover:bg-muted">
                    <TableHead className="text-foreground w-16">Pos</TableHead>
                    <TableHead className="text-foreground">Equipo</TableHead>
                    <TableHead className="text-foreground">Tiempo</TableHead>
                    <TableHead className="text-foreground">Penal</TableHead>
                    <TableHead className="text-foreground">Total</TableHead>
                    <TableHead className="text-foreground">Estado</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {sortedResults.length === 0 ? (
                    <TableRow>
                      <TableCell colSpan={6} className="text-center py-8 text-muted-foreground">
                        No hay resultados capturados aún
                      </TableCell>
                    </TableRow>
                  ) : (
                    sortedResults.map((team, index) => (
                      <TableRow key={team.id} className="hover:bg-accent/30">
                        <TableCell>
                          <Badge
                            variant="outline"
                            className={[
                              index === 0 ? 'border-yellow-400 text-yellow-600' : '',
                              index === 1 ? 'border-gray-400 text-gray-600' : '',
                              index === 2 ? 'border-orange-400 text-orange-600' : '',
                            ]
                              .filter(Boolean)
                              .join(' ')}
                          >
                            {index + 1}
                          </Badge>
                        </TableCell>
                        <TableCell>
                          <div>
                            <div className="text-foreground">{team.header}</div>
                            <div className="text-muted-foreground">{team.heeler}</div>
                          </div>
                        </TableCell>
                        <TableCell>
                          {team.time !== null ? (
                            <span className="text-emerald-600 tabular-nums">{formatDisplayTime(team.time)}s</span>
                          ) : (
                            <span className="text-muted-foreground">—</span>
                          )}
                        </TableCell>
                        <TableCell>
                          {team.penalty > 0 ? (
                            <Badge className="bg-yellow-50 text-yellow-700 border-yellow-200">+{team.penalty}s</Badge>
                          ) : (
                            <span className="text-muted-foreground">—</span>
                          )}
                        </TableCell>
                        <TableCell>
                          {team.total !== null ? (
                            <span className="text-foreground tabular-nums">{formatDisplayTime(team.total)}s</span>
                          ) : team.status === 'nt' ? (
                            <span className="text-red-600">NT</span>
                          ) : team.status === 'dq' ? (
                            <span className="text-red-600">DQ</span>
                          ) : (
                            <span className="text-muted-foreground">—</span>
                          )}
                        </TableCell>
                        <TableCell>
                          {team.status === 'completed' && (
                            <Badge className="bg-emerald-50 text-emerald-700 border-emerald-200">✅ Calificado</Badge>
                          )}
                          {team.status === 'nt' && (
                            <Badge className="bg-red-50 text-red-700 border-red-200">❌ No Time</Badge>
                          )}
                          {team.status === 'dq' && (
                            <Badge className="bg-red-50 text-red-700 border-red-200">❌ Descalificado</Badge>
                          )}
                          {team.penalty > 0 && team.status === 'completed' && (
                            <Badge className="bg-yellow-50 text-yellow-700 border-yellow-200 ml-2">⚠ Penal</Badge>
                          )}
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
    </div>
  )
}
