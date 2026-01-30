import { useState, useEffect, useCallback } from 'react'
import {
  Play, Pause, RotateCcw, Save, ChevronLeft, ChevronRight, X, Clock,
  CheckCircle2, Activity, Lock, Users,
} from 'lucide-react'
import { Button } from './ui/button'
import { Input } from './ui/input'
import { Label } from './ui/label'
import { Checkbox } from './ui/checkbox'
import { Badge } from './ui/badge'
import { Switch } from './ui/switch'
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from './ui/select'
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from './ui/table'
import { Tabs, TabsContent, TabsList, TabsTrigger } from './ui/tabs'
import { toast } from 'sonner'
import { getRunsExpanded, saveRun, getStandings, updateEventStatus, generateDraw } from '../lib/api'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "./ui/alert-dialog"

import type { Event, Team as TeamType, Run as RunType } from '../types'

interface CaptureRunsTabProps {
  event: Event
  isLocked: boolean
  onLock: () => void
}

// RoundResult and GlobalStanding are view-specific shapes derived from Run/Team
interface RoundResult {
  position: number
  team: TeamType
  time: number | null
  penalty: number
  total: number | null
  status: 'valid' | 'penalty' | 'nt' | 'dq'
}

interface GlobalStanding {
  position: number
  team: TeamType
  roundsCompleted: number
  totalTime: number | null
  average: number | null
  status: 'qualified' | 'warning' | 'eliminated'
}

export function CaptureRunsTab({ event, isLocked, onLock }: CaptureRunsTabProps) {
  const [selectedRound, setSelectedRound] = useState('1')
  const [runs, setRuns] = useState<RunType[]>([])
  const [selectedTeamIndex, setSelectedTeamIndex] = useState<number | null>(null)
  const [timerRunning, setTimerRunning] = useState(false)
  const [timerValue, setTimerValue] = useState(0)
  const [penalty, setPenalty] = useState('0')
  const [noTime, setNoTime] = useState(false)
  const [dq, setDq] = useState(false)
  const [isManualMode, setIsManualMode] = useState(false)
  const [manualTimeInput, setManualTimeInput] = useState('')
  const [globalStandings, setGlobalStandings] = useState<GlobalStanding[]>([])
  const [isConfirmOpen, setIsConfirmOpen] = useState(false)
  const [inputPin, setInputPin] = useState('')
  const [pinError, setPinError] = useState(false)

  const totalRounds = event?.rounds ?? 3
  const currentRun = selectedTeamIndex !== null ? runs[selectedTeamIndex] : null

  const fetchRuns = useCallback(async () => {
    if (!event?.id) return
    try {
      const data = await getRunsExpanded(Number(event.id), Number(selectedRound))
      
      const filtered = data.filter((r: any) => r.status !== 'skipped');

      const mapped: RunType[] = filtered.map((r: any) => ({
        id: String(r.id),
        teamId: r.team_id,
        team: {
          id: r.team_id,
          header: r.header_name,
          heeler: r.heeler_name,
        },
        round: r.round,
        position: r.position,
        time: r.time_sec,
        penalty: r.penalty,
        noTime: !!r.no_time,
        dq: !!r.dq,
        status: r.status === 'completed' ? 'completed' : 'pending',
      }))
      setRuns(mapped)
    } catch (error) {
      console.error('Error fetching runs:', error)
      toast.error('Error al cargar los runs')
    }
  }, [event?.id, selectedRound])

  const fetchStandingsData = useCallback(async () => {
    if (!event?.id) return
    try {
      const data = await getStandings(Number(event.id))
      // Need to fetch team names for standings as getStandings returns team_id
      // For now, we can try to map from existing runs if we have them, or we might need to fetch teams.
      // Actually getStandings returns team_id. We can use the runs we have to find team names, 
      // or better, update getStandings to return names. 
      // For this iteration, let's assume we can find names from the runs list if loaded, 
      // or we might show ID if not found. 
      // A better approach: fetch teams list once or update getStandings.
      // Let's use a simple lookup from the current runs for now.
      
      const mapped: GlobalStanding[] = data.map((s: any) => {
        // Try to find team name from current runs
        const foundRun = runs.find(r => r.teamId === s.team_id)
        const teamName = foundRun ? foundRun.team : { id: s.team_id, header: 'Unknown', heeler: 'Unknown' }
        
        return {
          position: s.rank,
          team: teamName,
          roundsCompleted: s.completed_runs,
          totalTime: s.total_time,
          average: s.avg_time,
          status: 'qualified' // Placeholder logic
        }
      })
      setGlobalStandings(mapped)
    } catch (error) {
      console.error('Error fetching standings:', error)
    }
  }, [event?.id, runs])

  useEffect(() => {
    fetchRuns()
  }, [fetchRuns])

  useEffect(() => {
    if (runs.length > 0) {
        fetchStandingsData()
    }
  }, [runs, fetchStandingsData])


  // Timer
  useEffect(() => {
    let interval: ReturnType<typeof setInterval> | undefined
    if (timerRunning) {
      interval = setInterval(() => setTimerValue((prev) => prev + 10), 10)
    }
    return () => {
      if (interval) clearInterval(interval)
    }
  }, [timerRunning])

  // Shortcuts
  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (!currentRun) return
      switch (e.key) {
        case ' ':
          e.preventDefault()
          handleStartStop()
          break
        case 'Enter':
          e.preventDefault()
          handleSaveRun()
          break
        case 'r':
        case 'R':
          e.preventDefault()
          handleReset()
          break
        case 'n':
        case 'N':
          e.preventDefault()
          setNoTime(prev => {
            const newValue = !prev
            if (newValue) setDq(false)
            return newValue
          })
          break
        case 'ArrowRight':
          e.preventDefault()
          handleNext()
          break
        case 'p':
        case 'P':
        case 'ArrowLeft':
          e.preventDefault()
          handlePrevious()
          break
        case 'Escape':
          e.preventDefault()
          handleCloseCapture()
          break
      }
    }
    window.addEventListener('keydown', handleKey)
    return () => window.removeEventListener('keydown', handleKey)
  }, [currentRun, timerRunning, timerValue, penalty, noTime, dq])

  const formatTime = (ms: number) => {
    const m = Math.floor(ms / 60000)
    const s = Math.floor((ms % 60000) / 1000)
    const ms3 = Math.floor(ms % 1000)
    return `${m}:${s.toString().padStart(2, '0')}.${ms3.toString().padStart(3, '0')}`
  }

  const handleRoundChange = async (round: string) => {
    setSelectedRound(round)
    setSelectedTeamIndex(null)
    handleReset()
    
    // Auto-generate/Regenerate logic
    if (event?.id) {
       try {
          // Attempt to generate/regenerate the draw for this round automatically.
          // The backend will block this if the round already has started (has completed runs), so it's safe to call.
          // If it succeeds, we get a fresh draw without eliminated teams.
          // If it fails (round started), we just catch the error and load what exists.
          // Note: We only want to do this for rounds > 1 usually, but it's safe for R1 too if empty.
          if (parseInt(round) > 1) {
              await generateDraw({
                  event_id: Number(event.id),
                  round: parseInt(round),
                  reseed: true,    // Always shuffle new rounds? Or maybe preserve order? User asked for fluid.
                  seed_runs: true
              })
              toast.success(`Ronda ${round} preparada y filtrada.`)
          }
       } catch (e) {
          // Ignore "round started" errors silently, as that just means we are viewing history
          console.log('Auto-generation skipped:', e)
       }
    }
  }

  const handleSelectTeam = (index: number) => {
    setSelectedTeamIndex(index)
    const run = runs[index]
    // If run is completed, load its data? 
    // For now, we reset for new capture, or we could load existing time if we want to edit.
    // Let's just reset for now as per original behavior, or maybe load if completed?
    // Original mock behavior was reset.
    handleReset()
    if (run.status === 'completed') {
        // Optional: Load existing data to edit
        if (run.time !== null) {
          setTimerValue(run.time * 1000)
          setManualTimeInput(run.time.toFixed(3))
        }
        setPenalty(String(run.penalty))
        setNoTime(run.noTime)
        setDq(run.dq)
    }
  }

  const handleStartStop = () => setTimerRunning((v) => !v)

  const handleReset = () => {
    setTimerRunning(false)
    setTimerValue(0)
    setPenalty('0')
    setNoTime(false)
    setDq(false)
    setManualTimeInput('')
  }

  const handleCloseCapture = () => {
    setSelectedTeamIndex(null)
    handleReset()
  }

  const handleSaveRun = async () => {
    if (!currentRun || !event?.id) return

    if (currentRun.status === 'completed') {
      setInputPin('')
      setPinError(false)
      setIsConfirmOpen(true)
      return
    }

    await performSave()
  }

  const performSave = async () => {
    if (!currentRun || !event?.id) return
    
    // Calcular tiempo según el modo
    let timeInSeconds: number
    if (isManualMode) {
      const manualTime = parseFloat(manualTimeInput.trim())
      console.log('Manual time input:', manualTimeInput, 'Parsed:', manualTime, 'isNaN:', isNaN(manualTime))
      
      if (!noTime && !dq) {
        if (isNaN(manualTime) || manualTime <= 0) {
          toast.error('Ingresa un tiempo válido mayor a 0')
          return
        }
      }
      timeInSeconds = isNaN(manualTime) ? 0 : manualTime
    } else {
      timeInSeconds = timerValue / 1000
    }
    
    const penaltyValue = parseFloat(penalty) || 0

    try {
        await saveRun({
            event_id: Number(event.id),
            team_id: currentRun.teamId,
            round: currentRun.round,
            position: currentRun.position,
            time_sec: (noTime || dq) ? null : timeInSeconds,
            penalty: penaltyValue,
            no_time: noTime,
            dq: dq,
            captured_by: null // TODO: Add user ID if auth exists
        })

        // Lock event if not locked yet
        if (!isLocked) {
            await updateEventStatus(Number(event.id), 'locked')
            onLock()
            toast.success('Run guardado', { description: 'Evento bloqueado: no se puede regenerar draw.' })
        } else {
            toast.success('Run guardado', { description: (noTime || dq) ? 'Equipo eliminado de rondas siguientes.' : undefined })
        }

        // Re-fetch to apply "skipped" filtering if applicable
        await fetchRuns()
        
        // Refresh standings
        fetchStandingsData()
        
        // Move to next if exists
        handleNext()
        
        setIsConfirmOpen(false)
    } catch (error) {
        console.error('Error saving run:', error)
        toast.error('Error al guardar el run')
    }
  }

  const handleConfirmOverwrite = (e: React.MouseEvent) => {
    // If PIN is required
    if (event.adminPin) {
      if (inputPin !== event.adminPin) {
        e.preventDefault()
        setPinError(true)
        return
      }
    }
    performSave()
  }

  const handleNext = () => {
    const next = (selectedTeamIndex ?? -1) + 1
    if (next < runs.length) {
      handleSelectTeam(next)
    } else {
      handleCloseCapture()
    }
  }

  const handlePrevious = () => {
    const prev = (selectedTeamIndex ?? 0) - 1
    if (prev >= 0) {
      handleSelectTeam(prev)
    }
  }

  // Round results
  const roundResults: RoundResult[] = runs
    .filter((r) => r.status === 'completed')
    .map((run, i) => {
      let status: RoundResult['status'] = 'valid'
      if (run.dq) status = 'dq'
      else if (run.noTime) status = 'nt'
      else if (run.penalty > 0) status = 'penalty'
      const total = run.time !== null ? run.time + run.penalty : null
      return { position: i + 1, team: run.team, time: run.time, penalty: run.penalty, total, status }
    })
    .sort((a, b) => {
      if (a.status === 'dq' || a.status === 'nt') return 1
      if (b.status === 'dq' || b.status === 'nt') return -1
      if (a.total === null) return 1
      if (b.total === null) return -1
      return a.total - b.total
    })
    // Re-assign position based on sort
    .map((r, i) => ({ ...r, position: i + 1 }))

  const handleRecaptureClick = (index: number) => {
    // Just recapture directly
    handleSelectTeam(index)
  }

  // removed handleRecaptureConfirm

  return (
    <div className="h-full flex flex-col bg-background">
      {/* Header - Minimalist, inside content area (since we are in a tab) */}
      <div className="mb-6 flex items-center justify-between">
        <div className="flex items-center gap-4">
             <div className="flex items-center gap-2 bg-card border border-border rounded-xl p-1 px-3 shadow-sm">
                <Label className="text-muted-foreground whitespace-nowrap">Ronda actual:</Label>
                <Select value={selectedRound} onValueChange={handleRoundChange}>
                  <SelectTrigger className="w-[140px] border-none shadow-none h-8 font-medium">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {Array.from({ length: totalRounds }, (_, i) => (
                      <SelectItem key={i + 1} value={`${i + 1}`}>
                        Ronda {i + 1}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
             </div>
             
             {isLocked && (
                <Badge className="bg-accent text-primary border-accent animate-in fade-in">
                    <Lock className="mr-1 h-3 w-3" /> Locked
                </Badge>
             )}
        </div>
      </div>

      <div className="flex-1 flex gap-6 overflow-hidden min-h-0">
        {/* LEFT: Teams list */}
        <div className="w-96 bg-card border border-border rounded-xl flex flex-col overflow-hidden shadow-sm">
          <div className="p-4 border-b border-border bg-muted/30">
            <h3 className="text-foreground font-medium flex items-center gap-2">
                <Users className="h-4 w-4 text-muted-foreground" />
                Equipos
            </h3>
          </div>

          <div className="flex-1 overflow-auto">
            <Table>
                <TableHeader className="sticky top-0 bg-card z-10">
                  <TableRow className="hover:bg-card border-b border-border">
                  <TableHead className="text-foreground w-12 font-medium">#</TableHead>
                  <TableHead className="text-foreground font-medium">Equipo</TableHead>
                  <TableHead className="text-foreground w-16 text-center font-medium">Est.</TableHead>
                  <TableHead className="text-foreground w-20 text-right font-medium">Acción</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {runs.map((run, index) => (
                  <TableRow
                    key={run.id}
                    className={`cursor-pointer transition-colors ${
                      index === selectedTeamIndex ? 'bg-primary/5 hover:bg-primary/10' : 'hover:bg-muted/50'
                    }`}
                    onClick={() => handleSelectTeam(index)}
                  >
                    <TableCell className="font-medium text-foreground/80">{run.position}</TableCell>
                    <TableCell>
                      <div className="flex flex-col">
                        <span className="text-foreground font-medium leading-none">{run.team.header}</span>
                        <span className="text-muted-foreground text-xs mt-1">{run.team.heeler}</span>
                      </div>
                    </TableCell>
                    <TableCell className="text-center p-0">
                      {run.status === 'completed' ? (
                        <div className="inline-flex items-center justify-center w-6 h-6 rounded-full bg-emerald-100 text-emerald-600">
                            <CheckCircle2 className="w-4 h-4" />
                        </div>
                      ) : run.status === 'skipped' ? (
                         <div className="inline-flex items-center justify-center w-6 h-6 rounded-full bg-muted text-muted-foreground">
                            •
                        </div>
                      ) : (
                         <div className="inline-flex items-center justify-center w-6 h-6 rounded-full bg-blue-50 text-blue-500">
                            <div className="w-2 h-2 bg-current rounded-full" />
                        </div>
                      )}
                    </TableCell>
                    <TableCell className="text-right">
                      {run.status === 'completed' ? (
                        <Button
                          size="sm"
                          variant="ghost"
                          onClick={(e) => {
                            e.stopPropagation()
                            handleRecaptureClick(index)
                          }}
                          className="h-7 px-2 text-amber-600 hover:text-amber-700 hover:bg-amber-50 text-xs"
                        >
                          Editar
                        </Button>
                      ) : (
                        <Button
                          size="sm"
                          variant={index === selectedTeamIndex ? 'default' : 'secondary'}
                          onClick={(e) => {
                            e.stopPropagation()
                            handleSelectTeam(index)
                          }}
                          className={`h-7 px-3 text-xs ${
                            index === selectedTeamIndex 
                                ? 'bg-primary text-primary-foreground shadow-sm' 
                                : 'bg-secondary text-secondary-foreground hover:bg-secondary/80'
                          }`}
                        >
                          Capturar
                        </Button>
                      )}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
          
           <div className="p-3 border-t border-border bg-muted/30 text-xs text-muted-foreground text-center">
            {runs.filter(r => r.status === 'completed').length} / {runs.length} Runs completados
          </div>
        </div>

        {/* RIGHT: Capture + Results */}
        <div className="flex-1 flex flex-col gap-6 overflow-y-auto min-h-0 pr-1">
          
          {/* Capture Panel */}
          {currentRun ? (
            <div className="bg-card rounded-xl border border-border p-6 shadow-sm animate-in slide-in-from-bottom-2 duration-300">
              <div className="flex items-center justify-between mb-6">
                <div>
                    <div className="flex items-center gap-2 mb-2">
                        <Badge variant="outline" className="bg-primary/5 text-primary border-primary/20">
                            Ronda {currentRun.round}
                        </Badge>
                        <Badge variant="secondary" className="text-muted-foreground">
                            Run #{currentRun.position}
                        </Badge>
                    </div>
                    <h2 className="text-2xl font-semibold text-foreground tracking-tight">
                        {currentRun.team.header} <span className="text-muted-foreground text-lg font-normal">&</span> {currentRun.team.heeler}
                    </h2>
                </div>
                <Button variant="ghost" size="icon" onClick={handleCloseCapture} className="text-muted-foreground hover:bg-destructive/10 hover:text-destructive rounded-full">
                  <X className="w-5 h-5" />
                </Button>
              </div>

              {/* Mode Toggle */}
              <div className="flex items-center justify-center gap-3 mb-6 p-4 bg-muted/30 rounded-xl border border-border/50">
                <Label htmlFor="mode-switch" className={`font-medium transition-colors ${
                  !isManualMode ? 'text-foreground' : 'text-muted-foreground'
                }`}>
                  Cronómetro
                </Label>
                <Switch
                  id="mode-switch"
                  checked={isManualMode}
                  onCheckedChange={(checked) => {
                    setIsManualMode(checked)
                    handleReset()
                  }}
                />
                <Label htmlFor="mode-switch" className={`font-medium transition-colors ${
                  isManualMode ? 'text-foreground' : 'text-muted-foreground'
                }`}>
                  Entrada Manual
                </Label>
              </div>

              <div className="grid grid-cols-1 lg:grid-cols-2 gap-8 mb-8">
                  {/* Timer Display or Manual Input */}
                  {!isManualMode ? (
                    <div className="bg-foreground rounded-2xl p-8 flex flex-col items-center justify-center shadow-inner relative overflow-hidden group">
                        <div className="absolute top-4 left-0 right-0 flex justify-center opacity-50">
                          <div className="flex items-center gap-2 text-background/60 text-xs font-mono uppercase tracking-widest">
                              <Clock className="w-3 h-3" /> Cronómetro
                          </div>
                        </div>
                        
                        <div className="text-5xl lg:text-7xl font-mono font-bold text-primary tracking-tighter tabular-nums z-10 selection:bg-primary selection:text-primary-foreground">
                          {formatTime(timerValue)}
                        </div>
                    </div>
                  ) : (
                    <div className="bg-foreground rounded-2xl p-8 flex flex-col items-center justify-center shadow-inner relative overflow-hidden min-h-[200px]">
                        <div className="absolute top-4 left-0 right-0 flex justify-center opacity-50">
                          <div className="flex items-center gap-2 text-background/60 text-xs font-mono uppercase tracking-widest">
                              <Clock className="w-3 h-3" /> Entrada Manual
                          </div>
                        </div>
                        
                        <div className="w-full flex flex-col items-center justify-center z-10 px-4">
                          <input
                            type="number"
                            step="0.001"
                            value={manualTimeInput}
                            onChange={(e) => setManualTimeInput(e.target.value)}
                            onKeyDown={(e) => {
                              if (e.key === 'Enter') {
                                e.preventDefault()
                                handleSaveRun()
                              }
                            }}
                            placeholder="0.000"
                            className="text-5xl lg:text-6xl font-mono font-bold text-center border-none bg-transparent text-primary tracking-tighter tabular-nums outline-none w-full [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none"
                          />
                          <p className="text-center text-background/60 text-xs font-mono mt-3 uppercase tracking-widest">segundos</p>
                        </div>
                    </div>
                  )}

                   {/* Controls */}
                   {!isManualMode ? (
                     <div className="flex flex-col justify-center gap-4">
                          <Button
                              onClick={handleStartStop}
                              className={`h-20 text-xl font-medium rounded-2xl shadow-sm transition-all duration-200 transform active:scale-[0.98] ${
                              timerRunning 
                                  ? 'bg-destructive hover:bg-destructive/90 text-destructive-foreground ring-4 ring-destructive/10' 
                                  : 'bg-emerald-600 hover:bg-emerald-700 text-white ring-4 ring-emerald-600/10'
                              }`}
                          >
                              {timerRunning ? (
                              <span className="flex items-center gap-3">
                                  <Pause className="w-8 h-8 fill-current" /> Pausar
                              </span>
                              ) : (
                              <span className="flex items-center gap-3">
                                  <Play className="w-8 h-8 fill-current" /> Iniciar
                              </span>
                              )}
                          </Button>
                          
                          <Button onClick={handleReset} variant="outline" className="h-14 text-base border-border hover:bg-accent hover:text-accent-foreground rounded-xl">
                              <RotateCcw className="w-5 h-5 mr-2 group-hover:rotate-180 transition-transform duration-500" />
                              Reiniciar (Reset)
                          </Button>
                     </div>
                   ) : (
                     <div className="flex flex-col justify-center gap-4">
                          <Button onClick={handleReset} variant="outline" className="h-14 text-base border-border hover:bg-accent hover:text-accent-foreground rounded-xl">
                              <RotateCcw className="w-5 h-5 mr-2" />
                              Limpiar
                          </Button>
                          <div className="p-4 bg-muted/50 rounded-xl border border-border/50 text-sm text-muted-foreground">
                            <p className="font-medium mb-1">Ingresa el tiempo manualmente</p>
                            <p className="text-xs">Formato: segundos con hasta 3 decimales (ej: 8.456)</p>
                          </div>
                     </div>
                   )}
              </div>

              {/* Validation Inputs */}
              <div className="grid grid-cols-1 md:grid-cols-3 gap-6 p-6 bg-muted/30 rounded-xl border border-border/50">
                  <div className="space-y-2">
                    <Label htmlFor="penalty" className="text-foreground font-medium">Penalización (s)</Label>
                    <div className="relative">
                        <Input
                        id="penalty"
                        type="number"
                        step="1"
                        value={penalty}
                        onChange={(e) => setPenalty(e.target.value)}
                        placeholder="0"
                        className="bg-background border-border h-12 text-lg font-mono ps-4"
                        />
                        <span className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground text-sm font-medium">sec</span>
                    </div>
                  </div>
                  
                  <div className="flex items-center p-3 bg-background border border-border rounded-xl">
                     <Checkbox
                      id="noTime"
                      checked={noTime}
                      onCheckedChange={(c) => {
                        const v = !!c
                        setNoTime(v)
                        if (v) setDq(false)
                      }}
                      className="w-5 h-5"
                    />
                    <Label htmlFor="noTime" className="cursor-pointer ml-3 flex-1 font-medium">NT (No Time)</Label>
                  </div>

                  <div className="flex items-center p-3 bg-background border border-border rounded-xl">
                    <Checkbox
                      id="dq"
                      checked={dq}
                      onCheckedChange={(c) => {
                        const v = !!c
                        setDq(v)
                        if (v) setNoTime(false)
                      }}
                      className="w-5 h-5"
                    />
                    <Label htmlFor="dq" className="cursor-pointer ml-3 flex-1 font-medium">DQ (Descalificado)</Label>
                  </div>
              </div>

              {/* Navigation Actions */}
              <div className="flex gap-3 mt-6">
                <Button
                  onClick={handlePrevious}
                  disabled={selectedTeamIndex === 0}
                  variant="outline"
                  className="w-32 rounded-xl h-12 border-border"
                >
                  <ChevronLeft className="w-4 h-4 mr-2" />
                  Anterior
                </Button>
                
                <Button onClick={handleSaveRun} className="flex-1 h-12 rounded-xl text-lg font-medium shadow-md bg-primary text-primary-foreground hover:opacity-90">
                  <Save className="w-5 h-5 mr-2" />
                  Guardar Resultado
                </Button>
                
                <Button
                  onClick={handleNext}
                  disabled={selectedTeamIndex === runs.length - 1}
                  variant="outline"
                  className="w-32 rounded-xl h-12 border-border"
                >
                  Siguiente
                  <ChevronRight className="w-4 h-4 ml-2" />
                </Button>
              </div>
            </div>
          ) : (
            <div className="bg-card rounded-xl border border-dashed border-border p-12 flex flex-col items-center justify-center h-[500px] shadow-sm text-center">
              <div className="w-20 h-20 bg-muted rounded-full flex items-center justify-center mb-6">
                 <Clock className="w-10 h-10 text-muted-foreground/50" />
              </div>
              <h3 className="text-xl font-semibold text-foreground mb-2">Listo para capturar</h3>
              <p className="text-muted-foreground max-w-sm mx-auto">
                Selecciona un equipo de la lista de la izquierda para comenzar el cronometraje y registro de tiempos.
              </p>
            </div>
          )}
          
          {/* Stats Bar */}
          <div className="grid grid-cols-3 gap-4">
               <div className="bg-card p-4 rounded-xl border border-border shadow-sm flex flex-col gap-1">
                   <span className="text-xs text-muted-foreground font-medium uppercase tracking-wider flex items-center gap-2">
                       <Activity className="w-3 h-3" /> Progreso
                   </span>
                   <span className="text-2xl font-semibold text-foreground">
                      {Math.round((runs.filter(r => r.status === 'completed').length / (runs.length || 1)) * 100)}%
                   </span>
               </div>
               <div className="bg-card p-4 rounded-xl border border-border shadow-sm flex flex-col gap-1">
                   <span className="text-xs text-muted-foreground font-medium uppercase tracking-wider flex items-center gap-2">
                       <Clock className="w-3 h-3" /> Promedio Ronda
                   </span>
                   <span className="text-2xl font-semibold text-foreground tabular-nums">
                      {(() => {
                          const validTimes = roundResults.filter(r => r.total !== null).map(r => r.total as number);
                          if (validTimes.length === 0) return '—';
                          const sum = validTimes.reduce((a, b) => a + b, 0);
                          return (sum / validTimes.length).toFixed(2) + 's';
                      })()}
                   </span>
               </div>
               <div className="bg-card p-4 rounded-xl border border-border shadow-sm flex flex-col gap-1">
                   <span className="text-xs text-muted-foreground font-medium uppercase tracking-wider flex items-center gap-2">
                       <CheckCircle2 className="w-3 h-3" /> Calificados
                   </span>
                   <span className="text-2xl font-semibold text-emerald-600">
                      {roundResults.filter(r => r.status === 'valid').length}
                   </span>
               </div>
          </div>

          {/* Results Tabs */}
          <div className="bg-card rounded-xl border border-border shadow-sm flex-1 flex flex-col overflow-hidden min-h-[400px]">
             <Tabs defaultValue="round" className="flex flex-col h-full">
            <div className="px-6 py-4 border-b border-border flex items-center justify-between bg-muted/10">
              <h3 className="font-semibold text-foreground">Resultados</h3>
              <TabsList className="bg-muted">
                <TabsTrigger value="round" className="data-[state=active]:bg-background data-[state=active]:shadow-sm">Ronda actual</TabsTrigger>
                <TabsTrigger value="global" className="data-[state=active]:bg-background data-[state=active]:shadow-sm">Standings globales</TabsTrigger>
              </TabsList>
            </div>

            <TabsContent value="round" className="flex-1 overflow-auto p-0 m-0">
              <Table>
                <TableHeader className="sticky top-0 bg-card z-10 shadow-sm">
                  <TableRow className="hover:bg-card border-b border-border bg-muted/20">
                    <TableHead className="text-foreground w-16 font-medium text-center">Pos</TableHead>
                    <TableHead className="text-foreground font-medium">Equipo</TableHead>
                    <TableHead className="text-foreground text-right font-medium">Tiempo</TableHead>
                    <TableHead className="text-foreground text-right font-medium">Penal</TableHead>
                    <TableHead className="text-foreground text-right font-medium">Total</TableHead>
                    <TableHead className="text-foreground w-24 text-center font-medium">Estado</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {roundResults.length === 0 ? (
                    <TableRow>
                      <TableCell colSpan={6} className="text-center text-muted-foreground py-12 flex flex-col items-center justify-center gap-2 h-40">
                         <div className="w-12 h-12 rounded-full bg-muted flex items-center justify-center">
                            <Clock className="w-6 h-6 text-muted-foreground/30" />
                         </div>
                        <p>No hay resultados capturados para esta ronda</p>
                      </TableCell>
                    </TableRow>
                  ) : (
                    roundResults.map((r, idx) => (
                      <TableRow key={r.team.id} className="hover:bg-muted/30 border-b border-border/50 last:border-0">
                        <TableCell className="text-center">
                          <Badge 
                            variant="outline" 
                            className={`
                                w-8 h-8 rounded-full p-0 flex items-center justify-center border
                                ${idx === 0 ? 'bg-yellow-50 text-yellow-700 border-yellow-200' : ''}
                                ${idx === 1 ? 'bg-gray-50 text-gray-700 border-gray-200' : ''}
                                ${idx === 2 ? 'bg-orange-50 text-orange-700 border-orange-200' : ''}
                                ${idx > 2 ? 'border-border text-muted-foreground' : ''}
                            `}
                          >
                            {r.position}
                          </Badge>
                        </TableCell>
                        <TableCell>
                          <div className="flex flex-col">
                            <span className="font-medium text-foreground">{r.team.header}</span>
                            <span className="text-muted-foreground text-sm">{r.team.heeler}</span>
                          </div>
                        </TableCell>
                        <TableCell className="text-right font-mono text-foreground/80">
                          {r.time !== null ? r.time.toFixed(2) + 's' : '—'}
                        </TableCell>
                        <TableCell className="text-right font-mono text-foreground/80">
                          {r.penalty > 0 ? <span className="text-amber-600 font-bold">+{r.penalty}</span> : '—'}
                        </TableCell>
                        <TableCell className="text-right font-mono font-medium text-lg">
                          {r.total !== null ? (
                            <span className="text-foreground">{r.total.toFixed(2)}s</span>
                          ) : (
                            <span className="text-muted-foreground">—</span>
                          )}
                        </TableCell>
                        <TableCell className="text-center">
                          {r.status === 'valid' && <Badge className="bg-emerald-50 text-emerald-700 border-emerald-200 hover:bg-emerald-100">Valid</Badge>}
                          {r.status === 'penalty' && <Badge className="bg-amber-50 text-amber-700 border-amber-200 hover:bg-amber-100">Penalty</Badge>}
                          {r.status === 'nt' && <Badge className="bg-red-50 text-red-700 border-red-200 hover:bg-red-100">NT</Badge>}
                          {r.status === 'dq' && <Badge className="bg-red-50 text-red-700 border-red-200 hover:bg-red-100">DQ</Badge>}
                        </TableCell>
                      </TableRow>
                    ))
                  )}
                </TableBody>
              </Table>
            </TabsContent>

            <TabsContent value="global" className="flex-1 overflow-auto p-0 m-0">
               <Table>
                <TableHeader className="sticky top-0 bg-card z-10 shadow-sm">
                  <TableRow className="hover:bg-card border-b border-border bg-muted/20">
                    <TableHead className="text-foreground w-16 font-medium text-center">Pos</TableHead>
                    <TableHead className="text-foreground font-medium">Equipo</TableHead>
                    <TableHead className="text-foreground text-center font-medium">Runs</TableHead>
                    <TableHead className="text-foreground text-right font-medium">Total</TableHead>
                    <TableHead className="text-foreground text-right font-medium">Promedio</TableHead>
                    <TableHead className="text-foreground w-24 text-center font-medium">Estado</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {globalStandings.map((s) => (
                    <TableRow key={s.team.id} className="hover:bg-muted/30 border-b border-border/50 last:border-0">
                      <TableCell className="text-center">
                         <Badge variant="outline" className="w-8 h-8 rounded-full p-0 flex items-center justify-center border-border">
                            {s.position}
                         </Badge>
                      </TableCell>
                      <TableCell>
                        <div className="flex flex-col">
                          <span className="font-medium text-foreground">{s.team.header}</span>
                          <span className="text-muted-foreground text-sm">{s.team.heeler}</span>
                        </div>
                      </TableCell>
                      <TableCell className="text-center font-medium text-foreground/80">
                        {s.roundsCompleted}
                      </TableCell>
                      <TableCell className="text-right font-mono font-medium text-lg">
                        {s.totalTime !== null ? s.totalTime.toFixed(2) + 's' : '—'}
                      </TableCell>
                      <TableCell className="text-right font-mono text-foreground/80">
                        {s.average !== null ? s.average.toFixed(2) + 's' : '—'}
                      </TableCell>
                      <TableCell className="text-center">
                        {s.status === 'qualified' && <Badge className="bg-emerald-50 text-emerald-700 border-emerald-200">En curso</Badge>}
                        {s.status === 'warning' && <Badge className="bg-amber-50 text-amber-700 border-amber-200">Riesgo</Badge>}
                        {s.status === 'eliminated' && <Badge className="bg-red-50 text-red-700 border-red-200">Eliminado</Badge>}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </TabsContent>
          </Tabs>
          </div>
        </div>
      </div>

      {/* Alert Dialog for Overwrite/PIN */}
      <AlertDialog open={isConfirmOpen} onOpenChange={setIsConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>¿Sobrescribir resultado?</AlertDialogTitle>
            <AlertDialogDescription>
              Este equipo ya tiene un tiempo registrado. 
              {event.adminPin 
                ? " Ingresa el PIN de administrador para confirmar la sobrescritura." 
                : " ¿Estás seguro de que deseas guardar este nuevo resultado y sobrescribir el anterior?"}
            </AlertDialogDescription>
          </AlertDialogHeader>
          
          {event.adminPin && (
            <div className="py-2">
               <Label htmlFor="pin-confirm">PIN de Administrador</Label>
               <Input 
                 id="pin-confirm"
                 type="password"
                 className={pinError ? "border-red-500" : ""}
                 value={inputPin}
                 onChange={(e) => {
                   setInputPin(e.target.value)
                   setPinError(false)
                 }}
                 placeholder="####"
                 maxLength={4}
               />
               {pinError && <p className="text-xs text-red-500 mt-1">PIN incorrecto</p>}
            </div>
          )}

          <AlertDialogFooter>
            <AlertDialogCancel>Cancelar</AlertDialogCancel>
            <AlertDialogAction onClick={handleConfirmOverwrite}>Confirmar</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
