import { useState, useMemo, useEffect } from 'react'
import { Button } from './ui/button'
import { Input } from './ui/input'
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
import { useTeams } from '@/hooks/useTeams'
import { useRopers } from '@/hooks/useRopers'
import { getEvents, getSeries, hardDeleteTeamsForEvent } from '@/lib/api'
import { toast } from 'sonner'
import { AddTeamModal } from './AddTeamModal'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from './ui/dialog'
import { Label } from './ui/label'
import { Zap, Plus, MoreVertical, Edit, Trash2, RefreshCw } from 'lucide-react'
import { Badge } from './ui/badge'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from './ui/dropdown-menu'
import { Alert, AlertDescription, AlertTitle } from './ui/alert'
import { Switch } from './ui/switch'

interface Exclusion {
  id: string
  roperName: string
  reason: string
  date: string
}

export function TeamsManagement() {
  const [seriesList, setSeriesList] = useState<any[]>([])
  const [eventsList, setEventsList] = useState<any[]>([])
  
  const [selectedSeriesId, setSelectedSeriesId] = useState<string>('all')
  const [selectedEventId, setSelectedEventId] = useState<string>('')

  // Load Series and Events
  useEffect(() => {
    const loadData = async () => {
      try {
        const s = await getSeries()
        setSeriesList(s || [])
        const e = await getEvents()
        setEventsList(e || [])
        
        // Default to first event if available and none selected
        if (e && e.length > 0 && !selectedEventId) {
            setSelectedEventId(String(e[0].id))
        }
      } catch (err) {
        console.error('Failed to load series/events', err)
      }
    }
    loadData()
  }, [])

  // Filter events based on selected series
  const filteredEvents = useMemo(() => {
    if (selectedSeriesId === 'all') return eventsList
    return eventsList.filter(e => String(e.series_id) === selectedSeriesId)
  }, [eventsList, selectedSeriesId])

  // Update selected event if it becomes invalid due to series filter
  useEffect(() => {
    if (selectedEventId && filteredEvents.length > 0) {
        const exists = filteredEvents.find(e => String(e.id) === selectedEventId)
        if (!exists) {
            setSelectedEventId(String(filteredEvents[0].id))
        }
    } else if (filteredEvents.length > 0 && !selectedEventId) {
        setSelectedEventId(String(filteredEvents[0].id))
    } else if (filteredEvents.length === 0) {
        setSelectedEventId('')
    }
  }, [filteredEvents, selectedEventId])

  const eventIdNum = Number(selectedEventId) || 0
  const { teams, loading, err, add, edit, remove, refresh } = useTeams(eventIdNum, false)
  const { ropers } = useRopers()

  // --- Modal States ---
  const [isCreateModalOpen, setIsCreateModalOpen] = useState(false)
  const [editingTeam, setEditingTeam] = useState<any | null>(null)
  const [isAutoCreateModalOpen, setIsAutoCreateModalOpen] = useState(false)
  const [autoCreateMode, setAutoCreateMode] = useState<'random' | 'balanced' | 'role'>('random')
  const [combinationsPerRoper, setCombinationsPerRoper] = useState(1)
  const [clearExisting, setClearExisting] = useState(true)
  // const [allowDuplicates, setAllowDuplicates] = useState(false)
  // const [avoidPreviousDuplicates, setAvoidPreviousDuplicates] = useState(true)
  // const [seed, setSeed] = useState('')
  // const [exclusions, setExclusions] = useState<Exclusion[]>([])
  // const [isExclusionsModalOpen, setIsExclusionsModalOpen] = useState(false)

  const selectedEvent = eventsList.find(e => String(e.id) === selectedEventId)
  const maxRating = selectedEvent?.max_team_rating ?? 0

  const enrichedTeams = useMemo(() => {
    return teams.map(t => {
      const header = ropers.find(r => r.id === t.header_id)
      const heeler = ropers.find(r => r.id === t.heeler_id)
      return {
        ...t,
        headerName: header ? `${header.firstName} ${header.lastName}` : `ID: ${t.header_id}`,
        heelerName: heeler ? `${heeler.firstName} ${heeler.lastName}` : `ID: ${t.heeler_id}`,
        headerRating: header?.rating ?? '?',
        heelerRating: heeler?.rating ?? '?',
        // status is already in 't' but we might want to normalize it
      }
    })
  }, [teams, ropers])

  // --- Handlers ---

  const handleOpenCreateModal = () => {
    if (!selectedEventId) {
        toast.error("Selecciona un evento primero")
        return
    }
    setEditingTeam(null)
    setIsCreateModalOpen(true)
  }

  const handleOpenEditModal = (team: any) => {
    setEditingTeam({
      id: String(team.id),
      header_id: Number(team.header_id),
      heeler_id: Number(team.heeler_id),
      headerRating: Math.round((team.rating || 0) / 2), // Approximation if individual ratings missing
      heelerRating: Math.round((team.rating || 0) / 2),
    })
    setIsCreateModalOpen(true)
  }

  const handleDeleteTeam = async (teamId: string) => {
    try {
      await remove(Number(teamId))
      toast.success('Equipo eliminado')
    } catch (e: any) {
      toast.error(String(e?.message ?? e))
    }
  }

  const handleAddOrUpdateTeam = async (payload: {
    header_id: number
    heeler_id: number
    headerRating: number
    heelerRating: number
  }) => {
    const { header_id, heeler_id, headerRating, heelerRating } = payload
    const teamRating = headerRating + heelerRating
    
    try {
      if (editingTeam && (editingTeam as any).id) {
        await edit({ id: Number((editingTeam as any).id), rating: teamRating })
        toast.success('Equipo actualizado')
      } else {
        await add({ header_id: Number(header_id), heeler_id: Number(heeler_id), rating: teamRating })
        toast.success('Equipo creado')
      }
      await refresh()
    } catch (e: any) {
      toast.error(String(e?.message ?? e))
    }

    setIsCreateModalOpen(false)
    setEditingTeam(null)
  }

  const handleAutoCreateTeams = async () => {
    if (!selectedEventId) return

    // Balanced or Random mode implementation
    if (autoCreateMode === 'balanced' || autoCreateMode === 'random') {
      try {
        // Hard delete existing if requested
        if (clearExisting) {
            try {
                await hardDeleteTeamsForEvent(eventIdNum)
                await new Promise((res) => setTimeout(res, 150))
                await refresh()
            } catch (errHard) {
                console.error('Failed to clear teams', errHard)
                toast.error('Error limpiando equipos previos')
            }
        }

        const all = ropers || []
        const headers = all.filter((r: any) => r.specialty === 'header' || r.specialty === 'both')
        const heelers = all.filter((r: any) => r.specialty === 'heeler' || r.specialty === 'both')

        if (autoCreateMode === 'balanced') {
            headers.sort((a: any, b: any) => (b.rating || 0) - (a.rating || 0))
            heelers.sort((a: any, b: any) => (a.rating || 0) - (b.rating || 0))
        } else {
            // Random shuffle
            headers.sort(() => Math.random() - 0.5)
            heelers.sort(() => Math.random() - 0.5)
        }

        const pairs: Array<{ header: any; heeler: any; teamRating: number }> = []
        const usedHeelerCount = new Map<number, number>()
        const usedHeaderCount = new Map<number, number>()
        const existingPairs = new Set<string>() // "headerId-heelerId"
        
        // Pre-fill existing pairs if we didn't clear
        if (!clearExisting) {
            teams.forEach(t => {
                existingPairs.add(`${t.header_id}-${t.heeler_id}`)
                usedHeaderCount.set(t.header_id, (usedHeaderCount.get(t.header_id) || 0) + 1)
                usedHeelerCount.set(t.heeler_id, (usedHeelerCount.get(t.heeler_id) || 0) + 1)
            })
        }

        const newExclusions: Exclusion[] = []
        const maxR = Number(maxRating)
        const maxEntries = combinationsPerRoper

        // Try to fill N entries for each header
        for (let i = 0; i < maxEntries; i++) {
            // Re-shuffle for random mode on each pass to avoid bias
            if (autoCreateMode === 'random') {
                headers.sort(() => Math.random() - 0.5)
                heelers.sort(() => Math.random() - 0.5)
            }

            for (const h of headers) {
                const hId = Number(h.id)
                if ((usedHeaderCount.get(hId) || 0) >= maxEntries) continue

                let matched: any = null
                
                // Try to find a heeler
                for (const candidate of heelers) {
                    const cId = Number(candidate.id)
                    
                    // Skip if heeler full
                    if ((usedHeelerCount.get(cId) || 0) >= maxEntries) continue
                    
                    // Skip if same person
                    if (hId === cId) continue
                    
                    // Skip if pair exists
                    if (existingPairs.has(`${hId}-${cId}`)) continue

                    const th = Number(h.rating || 0)
                    const tc = Number(candidate.rating || 0)
                    
                    if (th + tc <= maxR) {
                        matched = candidate
                        break
                    }
                }

                if (matched) {
                    const mId = Number(matched.id)
                    const rating = Number(h.rating || 0) + Number(matched.rating || 0)
                    
                    pairs.push({ header: h, heeler: matched, teamRating: rating })
                    
                    usedHeaderCount.set(hId, (usedHeaderCount.get(hId) || 0) + 1)
                    usedHeelerCount.set(mId, (usedHeelerCount.get(mId) || 0) + 1)
                    existingPairs.add(`${hId}-${mId}`)
                } else {
                    // Only log exclusion if it's the first attempt, to avoid spamming
                    if (i === 0) {
                        newExclusions.push({
                            id: Date.now().toString() + String(h.id),
                            roperName: `${h.firstName} ${h.lastName}`,
                            reason: 'Sin pareja válida dentro del max rating',
                            date: new Date().toISOString().split('T')[0],
                        })
                    }
                }
            }
        }

        let created = 0
        let failed = 0
        
        for (const p of pairs) {
          try {
            await add({ 
              header_id: Number(p.header.id), 
              heeler_id: Number(p.heeler.id), 
              rating: p.teamRating 
            }, { suppressRefresh: true }) // Suppress refresh per item for speed
            created++
          } catch (err: any) {
            failed++
          }
        }
        
        await refresh()

        // setExclusions(prev => [...prev, ...newExclusions])
        toast.success(`⚡ ${created} equipos creados`, {
          description: `${failed} fallados.`,
        })
      } catch (err: any) {
        toast.error(String(err?.message ?? err))
      } finally {
        setIsAutoCreateModalOpen(false)
      }
    } else {
        // Placeholder for other modes
        toast.info("Modo no implementado completamente en esta vista rápida")
        setIsAutoCreateModalOpen(false)
    }
  }


  return (
    <div className="p-6 h-full">
      <div className="max-w-full">
        <div className="mb-6 flex items-start justify-between">
          <div>
            <h1 className="text-2xl font-semibold text-foreground">Gestión de Equipos</h1>
            <p className="text-sm text-muted-foreground">Vista global para administrar equipos por evento</p>
          </div>

          <div className="flex items-center gap-3">
            <Button variant="ghost" size="icon" onClick={() => refresh()} title="Recargar">
                <RefreshCw className={`size-4 ${loading ? 'animate-spin' : ''}`} />
            </Button>
            <Button variant="outline" className="rounded-md" onClick={() => setIsAutoCreateModalOpen(true)} disabled={!selectedEventId}>
                <Zap className="size-4 mr-2" />
                Auto-crear
            </Button>
            <Button className="bg-primary text-primary-foreground rounded-md" onClick={handleOpenCreateModal} disabled={!selectedEventId}>
                <Plus className="size-4 mr-2" />
                Crear Equipo
            </Button>
          </div>
        </div>

        {err && (
            <Alert variant="destructive" className="mb-4">
                <AlertTitle>Error</AlertTitle>
                <AlertDescription>{err}</AlertDescription>
            </Alert>
        )}

        {/* Filters */}
        <div className="grid grid-cols-1 gap-4 md:grid-cols-3 mb-4">
          <Select value={selectedSeriesId} onValueChange={setSelectedSeriesId}>
            <SelectTrigger className="bg-card border-border">
                <SelectValue placeholder="Seleccionar Serie" />
            </SelectTrigger>
            <SelectContent>
                <SelectItem value="all">Todas las series</SelectItem>
                {seriesList.map(s => (
                    <SelectItem key={s.id} value={String(s.id)}>{s.name}</SelectItem>
                ))}
            </SelectContent>
          </Select>

          <Select value={selectedEventId} onValueChange={setSelectedEventId}>
            <SelectTrigger className="bg-card border-border">
                <SelectValue placeholder="Seleccionar Evento" />
            </SelectTrigger>
            <SelectContent>
                {filteredEvents.map(e => (
                    <SelectItem key={e.id} value={String(e.id)}>{e.name}</SelectItem>
                ))}
            </SelectContent>
          </Select>

          <div className="flex items-center gap-2">
            <Input placeholder="Buscar por nombre de roper..." />
          </div>
        </div>

        {/* Debug Info (Temporary) */}
        <div className="text-xs text-muted-foreground mb-2">
            Debug: EventID: {selectedEventId} | Teams: {teams.length} | Loading: {String(loading)}
        </div>

        {/* Summary panel */}
        <div className="bg-card border border-border rounded-xl p-4 mb-6">
          <div className="grid grid-cols-2 gap-4 md:grid-cols-6 items-center">
            <div className="col-span-2 md:col-span-1 text-sm text-muted-foreground">Evento<br/><span className="text-foreground">{selectedEvent?.name || '-'}</span></div>
            <div className="col-span-2 md:col-span-1 text-sm text-muted-foreground">Max Rating<br/><span className="text-foreground">{maxRating > 0 ? maxRating : '-'}</span></div>
            <div className="col-span-2 md:col-span-1 text-center">
              <div className="text-sm text-muted-foreground">Total Equipos</div>
              <div className="text-2xl font-semibold">{teams.length}</div>
            </div>
            <div className="col-span-2 md:col-span-1 text-center">
              <div className="text-sm text-muted-foreground">Ropers Disp.</div>
              <div className="text-2xl font-semibold">{ropers.length}</div>
            </div>
            {/* ... */}
          </div>
        </div>

        {/* Table */}
        <div className="bg-card border border-border rounded-xl p-4">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>ID</TableHead>
                <TableHead>Header</TableHead>
                <TableHead>Heeler</TableHead>
                <TableHead>Rating Header</TableHead>
                <TableHead>Rating Heeler</TableHead>
                <TableHead>Team Rating</TableHead>
                <TableHead>Estado</TableHead>
                <TableHead>Acciones</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {loading ? (
                 <TableRow><TableCell colSpan={8} className="text-center">Cargando...</TableCell></TableRow>
              ) : !selectedEventId ? (
                 <TableRow><TableCell colSpan={8} className="text-center">Selecciona un evento para ver los equipos</TableCell></TableRow>
              ) : enrichedTeams.length === 0 ? (
                 <TableRow><TableCell colSpan={8} className="text-center">No hay equipos registrados</TableCell></TableRow>
              ) : (
                enrichedTeams.map((t) => (
                <TableRow key={t.id}>
                  <TableCell className="font-medium">{t.id}</TableCell>
                  <TableCell>{t.headerName}</TableCell>
                  <TableCell>{t.heelerName}</TableCell>
                  <TableCell><span className="inline-flex items-center justify-center rounded-full bg-muted/20 px-2 py-1 text-xs">{t.headerRating}</span></TableCell>
                  <TableCell><span className="inline-flex items-center justify-center rounded-full bg-muted/20 px-2 py-1 text-xs">{t.heelerRating}</span></TableCell>
                  <TableCell><span className="inline-flex items-center justify-center rounded-full bg-muted/20 px-2 py-1 text-sm">{t.rating}</span></TableCell>
                  <TableCell>
                    <Badge variant={t.status === 'active' || t.status === 'valid' ? 'default' : 'destructive'}>
                        {t.status}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-right">
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <Button variant="ghost" size="sm">
                          <MoreVertical className="size-4" />
                        </Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        <DropdownMenuItem onClick={() => handleOpenEditModal(t)}>
                          <Edit className="size-4 mr-2" />
                          Editar
                        </DropdownMenuItem>
                        <DropdownMenuSeparator />
                        <DropdownMenuItem onClick={() => handleDeleteTeam(String(t.id))} className="text-red-600">
                          <Trash2 className="size-4 mr-2" />
                          Eliminar
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </TableCell>
                </TableRow>
              )))}
            </TableBody>
          </Table>
        </div>
      </div>

      {/* Modals */}
      <AddTeamModal
        isOpen={isCreateModalOpen}
        onClose={() => { setIsCreateModalOpen(false); setEditingTeam(null) }}
        roperOptions={ropers.map((r) => ({ id: r.id, label: `${r.firstName} ${r.lastName}`, rating: r.rating }))}
        onAddTeam={(data) => {
          handleAddOrUpdateTeam({
            header_id: data.header_id,
            heeler_id: data.heeler_id,
            headerRating: data.headerRating,
            heelerRating: data.heelerRating,
          })
        }}
        initialValue={
          editingTeam
            ? {
                header_id: (editingTeam as any).header_id,
                heeler_id: (editingTeam as any).heeler_id,
                headerRating: (editingTeam as any).headerRating,
                heelerRating: (editingTeam as any).heelerRating,
              }
            : undefined
        }
        maxRating={maxRating}
      />

      <Dialog open={isAutoCreateModalOpen} onOpenChange={setIsAutoCreateModalOpen}>
        <DialogContent className="sm:max-w-[500px]">
          <DialogHeader>
            <DialogTitle className="text-foreground flex items-center gap-2">
              <Zap className="size-5 text-primary" />
              Auto-crear equipos
            </DialogTitle>
            <DialogDescription>Genera equipos automáticamente para {selectedEvent?.name}</DialogDescription>
          </DialogHeader>

          <div className="space-y-4 py-4">
            <div className="space-y-2">
              <Label htmlFor="mode">Modo</Label>
              <Select value={autoCreateMode} onValueChange={(v: any) => setAutoCreateMode(v)}>
                <SelectTrigger id="mode" className="bg-card border-border">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="random">Random</SelectItem>
                  <SelectItem value="balanced">Balanced (rating)</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div className="grid grid-cols-2 gap-4">
                <div className="space-y-2">
                    <Label>Entradas por vaquero</Label>
                    <Input 
                        type="number" 
                        min={1} 
                        max={10} 
                        value={combinationsPerRoper} 
                        onChange={e => setCombinationsPerRoper(Number(e.target.value))} 
                    />
                </div>
                 <div className="flex items-center space-x-2 pt-8">
                    <Switch id="clear-mode" checked={clearExisting} onCheckedChange={setClearExisting} />
                    <Label htmlFor="clear-mode">Borrar existentes</Label>
                </div>
            </div>

            {/* Simplified options for now */}
            <div className="bg-accent rounded-xl p-4 border border-accent shadow-sm">
              <p className="text-sm text-primary">
                ⚡ Se generarán equipos automáticamente. (Modo Balanced recomendado)
              </p>
            </div>
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setIsAutoCreateModalOpen(false)}>Cancelar</Button>
            <Button onClick={handleAutoCreateTeams}>Generar</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

    </div>
  )
}

