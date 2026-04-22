import { useState, useEffect, useCallback, useRef } from 'react'
import {
  Play, Pause, RotateCcw, Save, ChevronLeft, ChevronRight, X, Clock,
  CheckCircle2, Activity, Lock, Users, Usb, Timer, Search, Maximize2, Minimize2,
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
import { 
  getRunsExpanded, saveRun, getStandings, updateEventStatus, generateDraw,
  listSerialPorts, connectTimer, disconnectTimer, isTimerConnected, startTimerCapture,
  type SerialPortInfo, type TimerEvent
} from '../lib/api'
import { listen } from '@tauri-apps/api/event'
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
  eliminatedRound: number | null
  eliminationReason: 'nt' | 'dq' | null
}

type CumulativeEntry =
  | { state: 'time'; total: number }
  | { state: 'nt'; total: null }
  | { state: 'dq'; total: null }

type CumulativeTotalsMap = Record<number, CumulativeEntry>

const mapRunRow = (r: any): RunType => ({
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
  status: r.status === 'completed' ? 'completed' : r.status === 'skipped' ? 'skipped' : 'pending',
})

const buildRunsList = (rows: any[]): RunType[] =>
  rows
    .filter((row: any) => row.status !== 'skipped')
    .map(mapRunRow)

const calculateCumulativeTotals = (runsList: RunType[], roundLimit: number): CumulativeTotalsMap => {
  const totals: CumulativeTotalsMap = {}

  runsList.forEach((run) => {
    if (run.round > roundLimit) return
    const existing = totals[run.teamId]

    if (run.dq) {
      totals[run.teamId] = { state: 'dq', total: null }
      return
    }

    if (run.noTime) {
      if (existing?.state === 'dq') return
      totals[run.teamId] = { state: 'nt', total: null }
      return
    }

    if (run.status !== 'completed' || run.time === null) return
    if (existing?.state === 'dq' || existing?.state === 'nt') return

    const previousTotal = existing?.state === 'time' ? existing.total : 0
    totals[run.teamId] = {
      state: 'time',
      total: previousTotal + run.time + run.penalty,
    }
  })

  return totals
}

export function CaptureRunsTab({ event, isLocked, onLock }: CaptureRunsTabProps) {
  const [selectedRound, setSelectedRound] = useState('1')
  const [runs, setRuns] = useState<RunType[]>([])
  const [selectedTeamIndex, setSelectedTeamIndex] = useState<number | null>(null)
  const [allRuns, setAllRuns] = useState<RunType[]>([])
  const [timerRunning, setTimerRunning] = useState(false)
  const [timerValue, setTimerValue] = useState(0)
  const [penalty, setPenalty] = useState('0')
  const [noTime, setNoTime] = useState(false)
  const [dq, setDq] = useState(false)
  const [isManualMode, setIsManualMode] = useState(false)
  const [manualTimeInput, setManualTimeInput] = useState('')
  const [globalStandings, setGlobalStandings] = useState<GlobalStanding[]>([])
  const [globalSearchQuery, setGlobalSearchQuery] = useState('')
  const [isConfirmOpen, setIsConfirmOpen] = useState(false)
  const [inputPin, setInputPin] = useState('')
  const [pinError, setPinError] = useState(false)
  const [cumulativeTotals, setCumulativeTotals] = useState<CumulativeTotalsMap>({})
  
  // External Timer (Polaris) State
  const [captureMode, setCaptureMode] = useState<'manual' | 'external'>('manual')
  const [serialPorts, setSerialPorts] = useState<SerialPortInfo[]>([])
  const [selectedPort, setSelectedPort] = useState<string>('')
  const [timerConnected, setTimerConnected] = useState(false)
  const [showTimerSettings, setShowTimerSettings] = useState(false)
  const [isFullscreen, setIsFullscreen] = useState(false)

  const totalRounds = event?.rounds ?? 3
  const currentRun = selectedTeamIndex !== null ? runs[selectedTeamIndex] : null

  // Ref map for auto-scrolling team rows
  const teamRowRefs = useRef<Map<number, HTMLTableRowElement>>(new Map())

  const fetchRuns = useCallback(async () => {
    if (!event?.id) return
    try {
      const roundNumber = Number(selectedRound)
      const [currentRoundData, allRoundsData] = await Promise.all([
        getRunsExpanded(Number(event.id), roundNumber),
        getRunsExpanded(Number(event.id)),
      ])

      const mappedCurrent = buildRunsList(currentRoundData)
      const mappedAll = buildRunsList(allRoundsData)

      setRuns(mappedCurrent)
      setAllRuns(mappedAll)
      setCumulativeTotals(calculateCumulativeTotals(mappedAll, roundNumber))
    } catch (error) {
      console.error('Error fetching runs:', error)
      toast.error('Error al cargar los runs')
      setCumulativeTotals({})
      setAllRuns([])
    }
  }, [event?.id, selectedRound])

  const fetchStandingsData = useCallback(async () => {
    if (!event?.id) return
    try {
      const data = await getStandings(Number(event.id))

      const eliminationMap = new Map<number, { round: number; reason: 'nt' | 'dq' }>()
      allRuns.forEach((run) => {
        if ((run.noTime || run.dq) && !eliminationMap.has(run.teamId)) {
          eliminationMap.set(run.teamId, {
            round: run.round,
            reason: run.dq ? 'dq' : 'nt',
          })
        }
      })

      const mapped: GlobalStanding[] = data.map((s: any) => {
        const elimination = eliminationMap.get(s.team_id)
        return {
          position: s.rank,
          team: {
            id: s.team_id,
            header: s.header_name,
            heeler: s.heeler_name,
          },
          roundsCompleted: s.completed_runs,
          totalTime: s.total_time,
          average: s.avg_time,
          eliminatedRound: elimination ? elimination.round : null,
          eliminationReason: elimination ? elimination.reason : null,
        }
      })
      setGlobalStandings(mapped)
    } catch (error) {
      console.error('Error fetching standings:', error)
    }
  }, [event?.id, allRuns])

  useEffect(() => {
    fetchRuns()
  }, [fetchRuns])

  useEffect(() => {
    if (!event?.id) return
    fetchStandingsData()
  }, [event?.id, allRuns, fetchStandingsData])


  // Timer (Manual Mode)
  useEffect(() => {
    let interval: ReturnType<typeof setInterval> | undefined
    if (timerRunning && captureMode === 'manual') {
      interval = setInterval(() => setTimerValue((prev) => prev + 10), 10)
    }
    return () => {
      if (interval) clearInterval(interval)
    }
  }, [timerRunning, captureMode])

  // External Timer Setup
  useEffect(() => {
    loadSerialPorts()
    checkTimerConnection()
  }, [])

  // Listen for timer events from backend
  useEffect(() => {
    if (captureMode !== 'external' || !timerConnected) return

    const unlisten = listen<TimerEvent>('timer-event', (event) => {
      const timerEvent = event.payload
      console.log('Timer event received:', timerEvent)
      
      // Auto-capture time
      const timeInMs = timerEvent.time_seconds * 1000
      setTimerValue(timeInMs)
      setManualTimeInput(timerEvent.time_seconds.toFixed(3))

      // Registrar automáticamente como NO time si el tiempo supera 15 segundos
      if (timerEvent.time_seconds > 15) {
        setNoTime(true)
        setDq(false)
        toast.warning(`NO TIME automático: ${timerEvent.time_seconds.toFixed(3)}s (límite 15s)`, {
          description: timerEvent.raw_text.trim()
        })
      } else {
        setNoTime(false)
        toast.success(`Tiempo capturado: ${timerEvent.time_seconds.toFixed(3)}s`, {
          description: timerEvent.raw_text.trim()
        })
      }
      
      // Optional: Auto-save if a team is selected
      if (currentRun && selectedTeamIndex !== null) {
        // Could auto-save here or let user confirm
        // For now, just capture the time
      }
    })

    return () => {
      unlisten.then(fn => fn())
    }
  }, [captureMode, timerConnected, currentRun, selectedTeamIndex])

  // Shortcuts
  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement)?.tagName
      if (tag === 'INPUT' || tag === 'TEXTAREA') return
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
          setIsFullscreen(false)
          handleCloseCapture()
          break
        case 'F5':
          e.preventDefault()
          setPenalty(prev => prev === '5' ? '0' : '5')
          break
        case 'F10':
          e.preventDefault()
          setPenalty(prev => prev === '10' ? '0' : '10')
          break
      }
    }
    window.addEventListener('keydown', handleKey)
    return () => window.removeEventListener('keydown', handleKey)
  }, [currentRun, timerRunning, timerValue, penalty, noTime, dq])

  // Auto-scroll the teams list to keep the active row visible
  useEffect(() => {
    if (selectedTeamIndex === null) return
    const el = teamRowRefs.current.get(selectedTeamIndex)
    if (el) {
      el.scrollIntoView({ behavior: 'smooth', block: 'nearest' })
    }
  }, [selectedTeamIndex])

  const formatTime = (ms: number) => {
    const m = Math.floor(ms / 60000)
    const s = Math.floor((ms % 60000) / 1000)
    const ms3 = Math.floor(ms % 1000)
    return `${m}:${s.toString().padStart(2, '0')}.${ms3.toString().padStart(3, '0')}`
  }

  const handleRoundChange = async (round: string) => {
    const roundNumber = parseInt(round)
    const isFinalRound = roundNumber === totalRounds

    // Reset selection state immediately
    setSelectedTeamIndex(null)
    handleReset()

    // For non-final rounds: the draw order is immutable (set once at batch generation).
    // Just navigate — the useEffect will trigger fetchRuns with the correct data.
    if (!event?.id || !isFinalRound) {
      setSelectedRound(round)
      return
    }

    // ── Final round selected ──────────────────────────────────────────────────
    // If the final round already exists in the BD (was previously generated), just navigate.
    const finalRoundExists = allRuns.some(r => r.round === roundNumber)
    if (finalRoundExists) {
      setSelectedRound(round)
      return
    }

    // Final round not yet generated: verify ALL previous rounds are fully captured.
    const prevPendingRuns = allRuns.filter(
      r => r.round < totalRounds && r.status === 'pending'
    )
    if (prevPendingRuns.length > 0) {
      toast.warning(
        `No se puede generar la ronda final aún: hay ${prevPendingRuns.length} run(s) pendiente(s) en rondas anteriores. Completa todos los runs primero.`
      )
      // Do not navigate — keep the user on the current round
      return
    }

    // All previous rounds fully captured: generate final round ordered by accumulated time.
    // setSelectedRound is called AFTER generateDraw so that fetchRuns (triggered by the state
    // change) reads the BD only once it already contains the newly generated final round data.
    try {
      await generateDraw({
        event_id: Number(event.id),
        round: roundNumber,
        reseed: false,
        seed_runs: true,
      })
      toast.success(`Ronda final ${round} generada. Los equipos se ordenaron de mayor a menor tiempo acumulado.`)
    } catch (e) {
      console.error('Error generating final round:', e)
      toast.error('Error al generar la ronda final: ' + String(e))
    }
    // Navigate regardless of success/failure (backend error means round may already exist)
    setSelectedRound(round)
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

  const handleStartStop = () => {
    setTimerRunning((running) => {
      // Al detener el cronómetro, verificar si supera 15 segundos
      if (running) {
        const seconds = timerValue / 1000
        if (seconds > 15) {
          setNoTime(true)
          setDq(false)
          toast.warning(`NO TIME automático: ${seconds.toFixed(3)}s (límite 15s)`)
        } else {
          setNoTime(false)
        }
      }
      return !running
    })
  }

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

    const requiresAdminPin = currentRun.status === 'completed' || currentRun.noTime || currentRun.dq
    if (requiresAdminPin) {
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

  // External Timer Functions
  const loadSerialPorts = async () => {
    try {
      const ports = await listSerialPorts()
      setSerialPorts(ports)
      if (ports.length > 0 && !selectedPort) {
        setSelectedPort(ports[0].port_name)
      }
    } catch (error) {
      console.error('Error loading serial ports:', error)
      toast.error('Error al listar puertos seriales')
    }
  }

  const checkTimerConnection = async () => {
    try {
      const connected = await isTimerConnected()
      setTimerConnected(connected)
    } catch (error) {
      console.error('Error checking timer connection:', error)
    }
  }

  const handleConnectTimer = async () => {
    if (!selectedPort) {
      toast.error('Selecciona un puerto serial')
      return
    }

    try {
      await connectTimer(selectedPort)
      await startTimerCapture()
      setTimerConnected(true)
      toast.success(`Timer conectado: ${selectedPort}`)
    } catch (error) {
      console.error('Error connecting timer:', error)
      toast.error('Error al conectar timer: ' + error)
    }
  }

  const handleDisconnectTimer = async () => {
    try {
      await disconnectTimer()
      setTimerConnected(false)
      toast.success('Timer desconectado')
    } catch (error) {
      console.error('Error disconnecting timer:', error)
      toast.error('Error al desconectar timer')
    }
  }

  const handleCaptureModeChange = (mode: 'manual' | 'external') => {
    setCaptureMode(mode)
    handleReset()
    if (mode === 'external' && !timerConnected) {
      setShowTimerSettings(true)
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

  const formatSignedSeconds = (value: number) => {
    if (value === 0) return '0.00s'
    const sign = value > 0 ? '+' : '-'
    return `${sign}${Math.abs(value).toFixed(2)}s`
  }

  const currentRoundNumber = Number(selectedRound)
  const currentRoundCompletedRuns = runs.filter((run) => run.status === 'completed').length
  const overallCompletedRuns = allRuns.filter((run) => run.status === 'completed').length
  const overallTotalRuns = allRuns.length
  const standingsComplete = overallTotalRuns > 0 && overallCompletedRuns >= overallTotalRuns
  const standingsProgress = overallTotalRuns > 0
    ? Math.min((overallCompletedRuns / overallTotalRuns) * 100, 100)
    : 0
  const leaderTime = globalStandings.length > 0 ? globalStandings[0].totalTime : null
  const currentRoundCompletedStandings = globalStandings.filter(
    (standing) =>
      standing.totalTime !== null &&
      !standing.eliminatedRound &&
      (standing.roundsCompleted || 0) >= currentRoundNumber,
  )
  const currentRoundLeaderTime = currentRoundCompletedStandings.reduce<number | null>((best, standing) => {
    if (standing.totalTime === null) return best
    if (best === null || standing.totalTime < best) return standing.totalTime
    return best
  }, null)
  const currentTeamPriorTotal = currentRun
    ? allRuns.reduce((sum, run) => {
        if (run.teamId !== currentRun.teamId) return sum
        if (run.round >= currentRun.round) return sum
        if (run.status !== 'completed' || run.time === null || run.noTime || run.dq) return sum
        return sum + run.time + run.penalty
      }, 0)
    : null
  const currentRequiredRunToLead = currentRun && currentRoundLeaderTime !== null && currentTeamPriorTotal !== null
    ? currentRoundLeaderTime - currentTeamPriorTotal - 0.001
    : null
  const isCompactView = !isFullscreen

  return (
    <div className={`flex flex-col bg-background ${isFullscreen ? 'fixed inset-0 z-50 p-4' : 'flex-1 min-h-0'}`}>
      {/* Header - Minimalist, inside content area (since we are in a tab) */}
      <div className="mb-3 flex items-center justify-between">
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
             
             {/* Capture Mode Selector */}
             <div className="flex items-center gap-2 bg-card border border-border rounded-xl p-1 px-3 shadow-sm">
                <Label className="text-muted-foreground whitespace-nowrap">Modo:</Label>
                <Select value={captureMode} onValueChange={(v) => handleCaptureModeChange(v as 'manual' | 'external')}>
                  <SelectTrigger className="w-[140px] border-none shadow-none h-8 font-medium">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="manual">
                      <div className="flex items-center gap-2">
                        <Clock className="h-4 w-4" />
                        <span>Manual</span>
                      </div>
                    </SelectItem>
                    <SelectItem value="external">
                      <div className="flex items-center gap-2">
                        <Usb className="h-4 w-4" />
                        <span>Timer Externo</span>
                      </div>
                    </SelectItem>
                  </SelectContent>
                </Select>
             </div>

             {/* Timer Status Badge */}
             {captureMode === 'external' && (
                <Badge 
                  variant={timerConnected ? "default" : "secondary"}
                  className={timerConnected ? "bg-green-500 text-white" : ""}
                >
                  {timerConnected ? (
                    <>
                      <Timer className="mr-1 h-3 w-3" /> Timer Conectado
                    </>
                  ) : (
                    <>
                      <Timer className="mr-1 h-3 w-3" /> Timer Desconectado
                    </>
                  )}
                </Badge>
             )}

             {/* Timer Settings Button */}
             {captureMode === 'external' && (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setShowTimerSettings(!showTimerSettings)}
                  className="h-8"
                >
                  <Usb className="mr-2 h-4 w-4" />
                  Configurar Timer
                </Button>
             )}
             
             {isLocked && (
                <Badge className="bg-accent text-primary border-accent animate-in fade-in">
                    <Lock className="mr-1 h-3 w-3" /> Bloqueado
                </Badge>
             )}
        </div>
        <button
          onClick={() => setIsFullscreen(f => !f)}
          className="ml-2 flex items-center justify-center w-8 h-8 rounded-lg border border-border bg-card text-muted-foreground hover:bg-muted transition-colors flex-shrink-0"
          title={isFullscreen ? 'Salir de pantalla completa' : 'Pantalla completa'}
        >
          {isFullscreen ? <Minimize2 className="w-4 h-4" /> : <Maximize2 className="w-4 h-4" />}
        </button>
      </div>

      {/* Timer Settings Panel */}
      {showTimerSettings && captureMode === 'external' && (
        <div className="mb-4 bg-card border border-border rounded-xl p-4 shadow-sm animate-in slide-in-from-top-2">
          <h3 className="text-lg font-semibold mb-4 flex items-center gap-2">
            <Usb className="h-5 w-5" />
            Configuración del Timer Polaris
          </h3>
          <div className="space-y-4">
            <div className="flex gap-4 items-end">
              <div className="flex-1">
                <Label htmlFor="serial-port">Puerto Serial (COM)</Label>
                <Select value={selectedPort} onValueChange={setSelectedPort}>
                  <SelectTrigger id="serial-port" className="w-full">
                    <SelectValue placeholder="Seleccionar puerto" />
                  </SelectTrigger>
                  <SelectContent>
                    {serialPorts.map((port) => (
                      <SelectItem key={port.port_name} value={port.port_name}>
                        {port.port_name} ({port.port_type})
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <p className="text-xs text-muted-foreground mt-1">
                  1200 baud, 8N1 - Compatible con FarmTek Polaris
                </p>
              </div>
              <Button
                variant="outline"
                onClick={loadSerialPorts}
                className="h-10"
              >
                Actualizar Lista
              </Button>
            </div>
            
            <div className="flex gap-2">
              {!timerConnected ? (
                <Button
                  onClick={handleConnectTimer}
                  disabled={!selectedPort}
                  className="bg-green-600 hover:bg-green-700"
                >
                  <Usb className="mr-2 h-4 w-4" />
                  Conectar Timer
                </Button>
              ) : (
                <Button
                  onClick={handleDisconnectTimer}
                  variant="destructive"
                >
                  Desconectar Timer
                </Button>
              )}
            </div>

            <div className="bg-muted/50 rounded-lg p-3 text-sm">
              <h4 className="font-medium mb-2">Instrucciones:</h4>
              <ol className="list-decimal list-inside space-y-1 text-muted-foreground">
                <li>Conecta el cable del Timer Console al puerto USB de tu computadora</li>
                <li>Selecciona el puerto COM correcto de la lista</li>
                <li>Haz clic en "Conectar Timer"</li>
                <li>Los tiempos se capturarán automáticamente cuando el timer se detenga</li>
              </ol>
            </div>
          </div>
        </div>
      )}

      <div className="flex-1 h-full flex gap-6 overflow-hidden min-h-0">
        {/* LEFT: Teams list */}
        <div className="w-96 h-full min-h-0 bg-card border border-border rounded-xl flex flex-col overflow-hidden shadow-sm">
          <div className="p-4 border-b border-border bg-muted/30">
            <h3 className="text-foreground font-medium flex items-center gap-2">
                <Users className="h-4 w-4 text-muted-foreground" />
                Equipos
            </h3>
          </div>

          <div className="flex-1 h-0 min-h-0 overflow-hidden [&>div]:h-full [&>div]:overflow-y-auto">
            <Table>
                <TableHeader className="sticky top-0 bg-card z-10">
                  <TableRow className="hover:bg-card border-b border-border">
                  <TableHead className="text-foreground w-12 font-medium">#</TableHead>
                  <TableHead className="text-foreground font-medium">Equipo</TableHead>
                  <TableHead className="text-foreground w-24 text-right font-medium">Acum.</TableHead>
                  <TableHead className="text-foreground w-16 text-center font-medium">Est.</TableHead>
                  <TableHead className="text-foreground w-20 text-right font-medium">Acción</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {runs.map((run, index) => {
                  const cumulative = cumulativeTotals[run.teamId]
                  return (
                    <TableRow
                      ref={(el) => { if (el) teamRowRefs.current.set(index, el); else teamRowRefs.current.delete(index) }}
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
                      <TableCell className="text-right align-middle">
                        {!cumulative ? (
                          <span className="text-xs text-muted-foreground">—</span>
                        ) : cumulative.state === 'time' ? (
                          <div className="flex flex-col items-end leading-tight">
                            <span className="font-mono text-sm font-semibold text-foreground">{cumulative.total.toFixed(2)}s</span>
                            <span className="text-[10px] uppercase tracking-wider text-muted-foreground/70">Acum.</span>
                          </div>
                        ) : (
                          <Badge
                            variant="outline"
                            className={`px-2 py-0.5 text-[11px] font-semibold uppercase tracking-wider ${
                              cumulative.state === 'dq'
                                ? 'bg-red-50 text-red-700 border-red-200'
                                : 'bg-amber-50 text-amber-700 border-amber-200'
                            }`}
                          >
                            {cumulative.state.toUpperCase()}
                          </Badge>
                        )}
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
                  )
                })}
              </TableBody>
            </Table>
          </div>
          
           <div className="p-3 border-t border-border bg-muted/30 text-xs text-muted-foreground text-center">
            {runs.filter(r => r.status === 'completed').length} / {runs.length} Runs completados
          </div>
        </div>

        {/* RIGHT: Capture + Results */}
        <div className="flex-1 flex flex-col overflow-hidden min-h-0 gap-4">

          {/* Upper: Capture panel (+ stats in fullscreen) */}
          <div className={`${isFullscreen ? 'flex-shrink-0 overflow-y-auto max-h-[50%]' : 'flex-1 min-h-0'} flex flex-col gap-4`}>

          {/* Capture Panel */}
          {currentRun ? (
            <div className={`bg-card rounded-xl border border-border shadow-sm animate-in slide-in-from-bottom-2 duration-300 overflow-hidden ${isCompactView ? 'flex-1 min-h-0' : ''}`}>

              {/* Header: badges + team name + mode toggle + close */}
              <div className="flex items-center gap-3 px-4 py-3 border-b border-border">
                <div className="flex items-center gap-2 flex-shrink-0">
                  <Badge variant="outline" className="bg-primary/5 text-primary border-primary/20">
                    Ronda {currentRun.round}
                  </Badge>
                  <Badge variant="secondary" className="text-muted-foreground">
                    Run #{currentRun.position}
                  </Badge>
                </div>
                <h2 className="flex-1 text-base font-semibold text-foreground tracking-tight truncate">
                  {currentRun.team.header} <span className="text-muted-foreground font-normal">&</span> {currentRun.team.heeler}
                </h2>
                {/* Mode toggle inline */}
                {captureMode === 'manual' && (
                  <div className="flex items-center gap-2 flex-shrink-0">
                    <Label htmlFor="mode-switch" className={`text-sm font-medium transition-colors ${!isManualMode ? 'text-foreground' : 'text-muted-foreground'}`}>
                      Cronómetro
                    </Label>
                    <Switch
                      id="mode-switch"
                      checked={isManualMode}
                      onCheckedChange={(checked) => { setIsManualMode(checked); handleReset() }}
                    />
                    <Label htmlFor="mode-switch" className={`text-sm font-medium transition-colors ${isManualMode ? 'text-foreground' : 'text-muted-foreground'}`}>
                      Manual
                    </Label>
                  </div>
                )}
                {captureMode === 'external' && (
                  <div className="flex items-center gap-1.5 text-sm text-muted-foreground flex-shrink-0">
                    <Timer className="h-4 w-4 text-primary" />
                    <span>{timerConnected ? 'Esperando Polaris...' : 'Timer no conectado'}</span>
                  </div>
                )}
                <Button variant="ghost" size="icon" onClick={handleCloseCapture} className="text-muted-foreground hover:bg-destructive/10 hover:text-destructive rounded-full flex-shrink-0 h-8 w-8">
                  <X className="w-4 h-4" />
                </Button>
              </div>

              {/* Timer + Buttons side by side */}
              <div className="flex gap-3 p-3 border-b border-border">
                {/* Timer display */}
                {captureMode === 'manual' && !isManualMode ? (
                  <div className="flex-1 bg-foreground rounded-xl flex flex-col items-center justify-center py-4 shadow-inner relative overflow-hidden">
                    <div className="absolute top-2 left-0 right-0 flex justify-center opacity-40">
                      <span className="flex items-center gap-1.5 text-background/60 text-[10px] font-mono uppercase tracking-widest">
                        <Clock className="w-2.5 h-2.5" /> Cronómetro
                      </span>
                    </div>
                    <div className="text-[38px] font-mono font-bold text-primary tracking-tighter tabular-nums">
                      {formatTime(timerValue)}
                    </div>
                  </div>
                ) : (
                  <div className="flex-1 bg-foreground rounded-xl flex flex-col items-center justify-center py-4 shadow-inner relative overflow-hidden">
                    <div className="absolute top-2 left-0 right-0 flex justify-center opacity-40">
                      <span className="flex items-center gap-1.5 text-background/60 text-[10px] font-mono uppercase tracking-widest">
                        <Clock className="w-2.5 h-2.5" /> {captureMode === 'external' ? 'Timer Externo' : 'Entrada Manual'}
                      </span>
                    </div>
                    <input
                      type="number"
                      step="0.001"
                      value={manualTimeInput}
                      onChange={(e) => {
                        const val = e.target.value
                        setManualTimeInput(val)
                        const parsed = parseFloat(val)
                        if (!isNaN(parsed) && parsed > 15) {
                          setNoTime(true); setDq(false)
                          toast.warning(`NO TIME automático: ${parsed.toFixed(3)}s (límite 15s)`)
                        } else if (!isNaN(parsed) && parsed > 0) {
                          setNoTime(false)
                        }
                      }}
                      onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); handleSaveRun() } }}
                      placeholder="0.000"
                      readOnly={captureMode === 'external'}
                      className="text-[38px] font-mono font-bold text-center border-none bg-transparent text-primary tracking-tighter tabular-nums outline-none w-full [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none"
                    />
                    <p className="text-background/50 text-[10px] font-mono uppercase tracking-widest mt-1">
                      {captureMode === 'external' ? 'Capturado desde Polaris' : 'segundos'}
                    </p>
                  </div>
                )}

                {/* Action buttons column */}
                <div className="flex flex-col gap-2 w-44">
                  {captureMode === 'manual' && !isManualMode ? (
                    <>
                      <Button
                        onClick={handleStartStop}
                        className={`flex-1 text-base font-medium rounded-xl shadow-sm transition-all duration-200 active:scale-[0.98] ${
                          timerRunning
                            ? 'bg-destructive hover:bg-destructive/90 text-destructive-foreground'
                            : 'bg-emerald-600 hover:bg-emerald-700 text-white'
                        }`}
                      >
                        {timerRunning ? (
                          <span className="flex items-center gap-2"><Pause className="w-5 h-5 fill-current" /> Pausar</span>
                        ) : (
                          <span className="flex items-center gap-2"><Play className="w-5 h-5 fill-current" /> Iniciar</span>
                        )}
                      </Button>
                      <Button onClick={handleReset} variant="outline" className="flex-1 text-sm border-border hover:bg-accent rounded-xl">
                        <RotateCcw className="w-4 h-4 mr-2" /> Reiniciar
                      </Button>
                    </>
                  ) : (
                    <>
                      <Button onClick={handleReset} variant="outline" className="flex-1 text-sm border-border hover:bg-accent rounded-xl">
                        <RotateCcw className="w-4 h-4 mr-2" /> Limpiar
                      </Button>
                      <div className="flex-1 flex items-center p-2 bg-muted/50 rounded-xl border border-border/50 text-xs text-muted-foreground">
                        Formato: segundos con hasta 3 decimales (ej: 8.456)
                      </div>
                    </>
                  )}
                </div>
              </div>

              {/* Penalties + flags */}
              <div className="flex items-center gap-3 px-4 py-2 border-b border-border flex-wrap">
                <span className="text-xs font-medium text-muted-foreground flex-shrink-0">Penalización</span>
                <Button
                  type="button"
                  onClick={() => setPenalty(prev => prev === '5' ? '0' : '5')}
                  variant="ghost"
                  className={`h-8 px-3 text-sm font-semibold rounded-xl transition-all focus-visible:ring-0 focus-visible:outline-none ${
                    penalty === '5'
                      ? 'bg-primary hover:bg-primary/90 !text-white hover:!text-white shadow-sm'
                      : 'bg-background border border-border text-foreground'
                  }`}
                >
                  +5s <span className="ml-1 text-[10px] opacity-60 font-normal">F5</span>
                </Button>
                <Button
                  type="button"
                  onClick={() => setPenalty(prev => prev === '10' ? '0' : '10')}
                  variant="ghost"
                  className={`h-8 px-3 text-sm font-semibold rounded-xl transition-all focus-visible:ring-0 focus-visible:outline-none ${
                    penalty === '10'
                      ? 'bg-primary hover:bg-primary/90 !text-white hover:!text-white shadow-sm'
                      : 'bg-background border border-border text-foreground'
                  }`}
                >
                  +10s <span className="ml-1 text-[10px] opacity-60 font-normal">F10</span>
                </Button>
                <div className="w-px h-5 bg-border flex-shrink-0" />
                <div className="flex items-center gap-2">
                  <Checkbox
                    id="noTime"
                    checked={noTime}
                    onCheckedChange={(c) => { const v = !!c; setNoTime(v); if (v) setDq(false) }}
                    className="w-4 h-4"
                  />
                  <Label htmlFor="noTime" className="cursor-pointer text-sm font-medium">NT (No time)</Label>
                </div>
                <div className="flex items-center gap-2">
                  <Checkbox
                    id="dq"
                    checked={dq}
                    onCheckedChange={(c) => { const v = !!c; setDq(v); if (v) setNoTime(false) }}
                    className="w-4 h-4"
                  />
                  <Label htmlFor="dq" className="cursor-pointer text-sm font-medium">DQ</Label>
                </div>
              </div>

              {/* Navigation Actions */}
              <div className="flex gap-3 p-3">
                <Button
                  onClick={handlePrevious}
                  disabled={selectedTeamIndex === 0}
                  variant="outline"
                  className="w-28 rounded-xl h-10 border-border text-sm"
                >
                  <ChevronLeft className="w-4 h-4 mr-1" /> Anterior
                </Button>
                <Button onClick={handleSaveRun} className="flex-1 h-10 rounded-xl text-base font-medium shadow-md bg-primary text-primary-foreground hover:opacity-90">
                  <Save className="w-4 h-4 mr-2" /> Guardar Resultado
                </Button>
                <Button
                  onClick={handleNext}
                  disabled={selectedTeamIndex === runs.length - 1}
                  variant="outline"
                  className="w-28 rounded-xl h-10 border-border text-sm"
                >
                  Siguiente <ChevronRight className="w-4 h-4 ml-1" />
                </Button>
              </div>
            </div>
          ) : (
            <div className={`bg-card rounded-xl border border-dashed border-border p-6 flex flex-col items-center justify-center shadow-sm text-center ${isCompactView ? 'flex-1 min-h-0' : 'min-h-[140px]'}`}>
              <div className="w-14 h-14 bg-muted rounded-full flex items-center justify-center mb-3">
                 <Clock className="w-7 h-7 text-muted-foreground/50" />
              </div>
              <h3 className="text-lg font-semibold text-foreground mb-1">Listo para capturar</h3>
              <p className="text-sm text-muted-foreground max-w-sm mx-auto">
                Selecciona un equipo de la lista de la izquierda para comenzar el cronometraje y registro de tiempos.
              </p>
            </div>
          )}
          
          {/* Stats Bar */}
          {isFullscreen && (
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
          )}
          </div>{/* end upper: capture + stats */}

          {/* Lower: Results — fullscreen only */}
          {isFullscreen && (
          <div className="flex flex-col">
          {/* Results Tabs */}
          <div className="bg-card rounded-xl border border-border shadow-sm flex flex-col overflow-hidden">
             <Tabs defaultValue="round" className="flex flex-col">
            <div className="px-6 py-4 border-b border-border flex items-center justify-between bg-muted/10 flex-shrink-0">
              <h3 className="font-semibold text-foreground">Resultados</h3>
              <div className="flex items-center gap-3">
                <TabsList className="bg-muted">
                  <TabsTrigger value="round" className="data-[state=active]:bg-background data-[state=active]:shadow-sm">Ronda actual</TabsTrigger>
                  <TabsTrigger value="global" className="data-[state=active]:bg-background data-[state=active]:shadow-sm">Standings globales</TabsTrigger>
                </TabsList>
                <div className="relative flex-shrink-0">
                  <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-muted-foreground/50" />
                  <Input
                    placeholder="Buscar equipo..."
                    value={globalSearchQuery}
                    onChange={(e) => setGlobalSearchQuery(e.target.value)}
                    className="pl-8 h-9 w-52 text-sm bg-background border-border/60 rounded-xl focus-visible:ring-1 focus-visible:ring-ring"
                  />
                </div>
              </div>
            </div>

            <TabsContent value="round" className="overflow-hidden flex flex-col p-0 m-0">
              <div className="max-h-[34vh] overflow-y-auto">
              <Table className="table-fixed w-full">
                <colgroup>
                  <col style={{width:'5%'}} />
                  <col style={{width:'36%'}} />
                  <col style={{width:'14%'}} />
                  <col style={{width:'12%'}} />
                  <col style={{width:'14%'}} />
                  <col style={{width:'19%'}} />
                </colgroup>
                <TableHeader className="sticky top-0 bg-card z-10 shadow-sm">
                  <TableRow className="hover:bg-card border-b border-border bg-muted/20">
                    <TableHead className="text-foreground font-medium text-center">Pos</TableHead>
                    <TableHead className="text-foreground font-medium">Equipo</TableHead>
                    <TableHead className="text-foreground text-right font-medium">Tiempo</TableHead>
                    <TableHead className="text-foreground text-right font-medium">Penal</TableHead>
                    <TableHead className="text-foreground text-right font-medium">Total</TableHead>
                    <TableHead className="text-foreground text-center font-medium">Estado</TableHead>
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
              </div>
            </TabsContent>

            <TabsContent value="global" className="overflow-hidden flex flex-col p-0 m-0">
              {/* ── Summary bar (single row) ── */}
              <div className="flex-shrink-0 flex items-center gap-3 px-4 py-2 border-b border-border flex-wrap">
                {/* Status badge */}
                <span className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-semibold whitespace-nowrap flex-shrink-0 ${
                  standingsComplete
                    ? 'bg-emerald-50 text-emerald-700 border border-emerald-200'
                    : 'bg-primary/10 text-primary border border-primary/20'
                }`}>
                  <span className={`w-1.5 h-1.5 rounded-full ${standingsComplete ? 'bg-emerald-500' : 'bg-primary'}`} />
                  {standingsComplete ? 'Completado' : 'En curso'}
                </span>
                <span className="text-xs font-semibold text-foreground tabular-nums whitespace-nowrap">
                  {Math.round(standingsProgress)}%
                </span>
                <div className="w-20 h-[3px] bg-muted rounded-full overflow-hidden">
                  <div
                    className={`h-full rounded-full transition-all ${
                      standingsComplete ? 'bg-emerald-500' : 'bg-primary'
                    }`}
                    style={{ width: `${Math.round(standingsProgress)}%` }}
                  />
                </div>
                <span className="text-xs font-medium text-foreground whitespace-nowrap">
                  Ronda <strong>{currentRoundNumber}</strong> de {totalRounds}
                </span>
                <span className="text-border">·</span>
                <span className="text-xs text-muted-foreground whitespace-nowrap">
                  Equipos <strong className="text-foreground">{globalStandings.length}</strong>
                </span>
                <span className="text-border">·</span>
                <span className="text-xs text-muted-foreground whitespace-nowrap">
                  Runs <strong className="text-foreground">{currentRoundCompletedRuns}</strong>
                </span>
                {globalStandings.length > 0 && globalStandings[0].totalTime !== null && (
                  <>
                    <span className="text-border">·</span>
                    <span className="text-xs text-muted-foreground whitespace-nowrap">
                      Líder{' '}
                      <strong className="text-primary">
                        {globalStandings[0].team.header} ({globalStandings[0].totalTime.toFixed(2)}s)
                      </strong>
                    </span>
                    {currentRun && !noTime && !dq && (
                      <>
                        <span className="text-border">·</span>
                        <span className="text-xs text-muted-foreground whitespace-nowrap">
                          Equipo actual{' '}
                          <strong className="text-primary">
                            {currentRun.team.header} & {currentRun.team.heeler}
                          </strong>{' '}
                          {currentRoundLeaderTime === null ? (
                            <strong className="text-muted-foreground">esperando primer run de la ronda</strong>
                          ) : currentRequiredRunToLead !== null && currentRequiredRunToLead <= 0 ? (
                            <strong className="text-rose-600">ya no alcanza al líder</strong>
                          ) : (
                            <>
                              necesita{' '}
                              <strong className="text-primary">
                                {currentRequiredRunToLead?.toFixed(3)}s
                              </strong>
                            </>
                          )}
                        </span>
                      </>
                    )}
                  </>
                )}
              </div>
              <div className="max-h-[34vh] overflow-y-auto">
               <Table className="table-fixed">
                <colgroup>
                  <col className="w-10" />
                  <col className="w-[35%]" />
                  <col className="w-[10%]" />
                  <col className="w-[15%]" />
                  <col className="w-[15%]" />
                  <col className="w-[18%]" />
                </colgroup>
                <TableHeader className="sticky top-0 bg-card z-10 shadow-sm">
                  <TableRow className="hover:bg-card border-b border-border bg-muted/20">
                    <TableHead className="text-foreground font-medium text-center">Pos</TableHead>
                    <TableHead className="text-foreground font-medium">Equipo</TableHead>
                    <TableHead className="text-foreground text-center font-medium">Runs</TableHead>
                    <TableHead className="text-foreground text-right font-medium">Total</TableHead>
                    <TableHead className="text-foreground text-right font-medium">Promedio</TableHead>
                    <TableHead className="text-foreground text-right font-medium">vs Líder</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {globalStandings.filter((s) => {
                    if (!globalSearchQuery) return true
                    const q = globalSearchQuery.toLowerCase()
                    return (
                      String(s.team.id).includes(q) ||
                      s.team.header.toLowerCase().includes(q) ||
                      s.team.heeler.toLowerCase().includes(q)
                    )
                  }).map((s) => (
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
                          {s.eliminatedRound ? (
                            <span className="mt-1 text-[11px] font-semibold text-rose-600 bg-rose-50 border border-rose-100 rounded-full px-2 py-0.5 w-fit">
                              Eliminado R{s.eliminatedRound}
                              {s.eliminationReason ? ` (${s.eliminationReason.toUpperCase()})` : ''}
                            </span>
                          ) : null}
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
                      <TableCell className="text-right">
                        {s.position === 1 ? (
                          <span className="inline-flex items-center px-2 py-0.5 rounded-full text-[11px] font-semibold bg-amber-50 text-amber-700 border border-amber-200">
                            Líder
                          </span>
                        ) : leaderTime !== null && s.totalTime !== null && !s.eliminatedRound ? (
                          <span className="font-mono text-sm font-medium text-rose-500">
                            {formatSignedSeconds(s.totalTime - leaderTime)}
                          </span>
                        ) : (
                          <span className="text-muted-foreground">—</span>
                        )}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
              </div>
            </TabsContent>
          </Tabs>
          </div>{/* end Results card */}
          </div>
          )}
        </div>{/* end RIGHT column */}
      </div>{/* end main content area */}

      {/* Alert Dialog for Overwrite/PIN */}
      <AlertDialog open={isConfirmOpen} onOpenChange={setIsConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {noTime || dq 
                ? `¿Marcar como ${noTime ? 'No Time' : 'Descalificado'}?`
                : '¿Sobrescribir resultado?'
              }
            </AlertDialogTitle>
            <AlertDialogDescription>
              {noTime 
                ? "Marcando este equipo como No Time. " 
                : dq 
                ? "Marcando este equipo como Descalificado. " 
                : "Este equipo ya tiene un tiempo registrado. "}
              {event.adminPin 
                ? "Ingresa el PIN de administrador para confirmar." 
                : "¿Estás seguro de que deseas continuar?"}
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
