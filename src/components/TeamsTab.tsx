import { useMemo, useState, useEffect } from 'react'
import {
  Plus, Edit, Trash2, Zap, Search, MoreVertical, Eye, AlertTriangle, CheckCircle, XCircle,
} from 'lucide-react'
import { Button } from './ui/button'
import { Input } from './ui/input'
import { Badge } from './ui/badge'
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from './ui/table'
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from './ui/select'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter,
} from './ui/dialog'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger,
} from './ui/dropdown-menu'
import { toast } from 'sonner'
import { AddTeamModal } from './AddTeamModal'
import { useTeams } from '@/hooks/useTeams'
import { hardDeleteTeamsForEvent, listTeams } from '@/lib/api'
import { useRopers } from '@/hooks/useRopers'

interface TeamsTabProps {
  event: any
  isLocked?: boolean
}

interface Team {
  id: string
  teamId: number
  eventId: string
  header: string
  heeler: string
  headerRating: number
  heelerRating: number
  teamRating: number
  status: 'valid' | 'exceeds' | 'incomplete'
  createdAt: string
}

interface Exclusion {
  id: string
  roperName: string
  reason: string
  date: string
}

// ropers will be loaded from DB via hook

export function TeamsTab({ event, isLocked }: TeamsTabProps) {
  const eventIdStr: string = String(event?.id ?? '')
  const eventIdNum: number = Number(event?.id ?? 0)
  const { teams, add, edit, remove, refresh } = useTeams(eventIdNum, !!isLocked)
  const { ropers } = useRopers()

  const [query, setQuery] = useState('')
  const [statusFilter, setStatusFilter] = useState<'all' | 'valid' | 'exceeds' | 'incomplete'>('all')
  const [sortBy, setSortBy] = useState<'teamId' | 'rating' | 'header'>('teamId')

  // Crear/Editar equipo
  const [isCreateModalOpen, setIsCreateModalOpen] = useState(false)
  const [editingTeam, setEditingTeam] = useState<any | null>(null)

  // Auto-crear equipos
  const [isAutoCreateModalOpen, setIsAutoCreateModalOpen] = useState(false)
  // Removed complex modes state variables as per requirement for "All against all"
  
  // Exclusiones
  const [isExclusionsModalOpen, setIsExclusionsModalOpen] = useState(false)
  // starts empty; auto-create will push exclusions when needed
  // const [exclusions, setExclusions] = useState<Exclusion[]>([])
  const [exclusions] = useState<Exclusion[]>([])

  const eventId: string = eventIdStr
  const maxRating: number = event?.max_team_rating ?? event?.maxRating ?? event?.maxTeamRating ?? 0

  const calculateTeamStatus = (headerRating: number, heelerRating: number): Team['status'] => {
    if (!headerRating || !heelerRating) return 'incomplete'
    const total = headerRating + heelerRating
    if (maxRating > 0 && total > maxRating) return 'exceeds'
    return 'valid'
  }

  // Map for fast roper lookup
  const roperMap = useMemo(() => {
    const map = new Map<number, any>()
    ropers.forEach(r => map.set(Number(r.id), r))
    return map
  }, [ropers])

  // NOTE: `useTeams(eventId)` ya carga solo los equipos del evento actual.
  // El filtro por `event_id` era redundante y en algunos casos causaba que
  // equipos válidos no se mostraran si el campo del row venía con otro nombre.
  // Por eso removemos ese filtro y confiamos en el hook para scoping.
  const filteredTeams = useMemo(() => {
    return (teams || [])
      .map(t => {
        // Hydrate with roper data for filtering/sorting
        const header = roperMap.get(Number(t.header_id))
        const heeler = roperMap.get(Number(t.heeler_id))
        
        // Compute status dynamically
        const headerR = Number(t.rating_header ?? t.headerRating ?? header?.rating ?? 0)
        const heelerR = Number(t.rating_heeler ?? t.heelerRating ?? heeler?.rating ?? 0)
        const totalR = Number(t.team_rating ?? t.rating ?? (headerR + heelerR))
        
        let computedStatus = 'valid'
        if (!header || !heeler) computedStatus = 'incomplete'
        else if (maxRating > 0 && totalR > maxRating) computedStatus = 'exceeds'
        
        return {
            ...t,
            headerName: header ? `${header.firstName} ${header.lastName}` : '',
            heelerName: heeler ? `${heeler.firstName} ${heeler.lastName}` : '',
            computedStatus
        }
      })
      .filter((t: any) => {
        if (!query) return true
        const q = query.toLowerCase()
        return String(t.header_id).includes(q) || 
               String(t.heeler_id).includes(q) || 
               String(t.id).includes(q) ||
               t.headerName.toLowerCase().includes(q) ||
               t.heelerName.toLowerCase().includes(q)
      })
      .filter((t: any) => {
        if (statusFilter === 'all') return true
        return t.computedStatus === statusFilter
      })
      .sort((a: any, b: any) => {
        if (sortBy === 'teamId') return a.id - b.id
        if (sortBy === 'rating') return (a.rating || 0) - (b.rating || 0)
        if (sortBy === 'header') return a.headerName.localeCompare(b.headerName)
        return 0
      })
  }, [teams, eventId, query, statusFilter, sortBy, roperMap, maxRating])

  const handleOpenCreateModal = () => {
    if (isLocked) {
      toast.error('Evento bloqueado. No puedes crear equipos.')
      return
    }
    setEditingTeam(null)
    setIsCreateModalOpen(true)
  }

  // Handlers

  // DEBUG: ensure we explicitly refresh once when the component mounts for the given eventId.
  // This helps detect cases where the hook's internal refresh isn't firing for any reason.
  useEffect(() => {
    if (!eventIdNum || eventIdNum <= 0) return
    (async () => {
      try {
        // eslint-disable-next-line no-console
        console.debug('[TeamsTab] manual-refresh start', { eventId: eventIdNum })
        // Also call API directly to see raw IPC result (bypass hook) for debugging
        try {
          // eslint-disable-next-line no-console
          console.debug('[TeamsTab] manual-debug: calling api.listTeams directly', { eventId: eventIdNum })
          const raw = await listTeams(eventIdNum)
          // eslint-disable-next-line no-console
          console.debug('[TeamsTab] manual-debug: api.listTeams raw', { eventId: eventIdNum, rawLength: Array.isArray(raw) ? raw.length : 0, rawSample: Array.isArray(raw) ? raw.slice(0,3) : raw })
        } catch (e) {
          // eslint-disable-next-line no-console
          console.error('[TeamsTab] manual-debug: api.listTeams error', e)
        }

        // await refresh and use its return value (normalized array) to log immediate result
        try {
          const normalized = await refresh()
          // eslint-disable-next-line no-console
          console.debug('[TeamsTab] manual-refresh done', { eventId: eventIdNum, teamsNow: Array.isArray(normalized) ? normalized.length : 0 })
        } catch (e) {
          // eslint-disable-next-line no-console
          console.error('[TeamsTab] manual-refresh error on refresh()', e)
        }
      } catch (err) {
        // eslint-disable-next-line no-console
        console.error('[TeamsTab] manual-refresh error', err)
      }
    })()
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [eventIdNum])

        // Helper: determine if a team is valid according to rules
        function isValidTeam(team: any, maxTeamRating: number) {
          if (!team) return false
          // must have header and heeler ids
          const hid = team.header_id ?? team.headerId ?? team.headerId
          const cid = team.heeler_id ?? team.heelerId ?? team.heelerId
          if (!hid || !cid) return false
          if (String(hid) === String(cid)) return false

          // lookup ratings from ropers list if available
          const headerR = ropers.find((x) => String(x.id) === String(hid))
          const heelerR = ropers.find((x) => String(x.id) === String(cid))

          const ratingHeader = Number(team.rating_header ?? team.headerRating ?? headerR?.rating ?? 0)
          const ratingHeeler = Number(team.rating_heeler ?? team.heelerRating ?? heelerR?.rating ?? 0)
          const teamRating = Number(team.team_rating ?? team.rating ?? (ratingHeader + ratingHeeler))

          if (team.status === 'excluded' || team.status === 'excluded') return false
          if (Number(maxTeamRating) > 0 && teamRating > Number(maxTeamRating ?? 0)) return false
          return true
        }

        // derive metrics from teams for the current event
        const eventTeams = (teams || []).filter((t) => String(t.event_id) === eventId)
        const maxTeamRating = event?.max_team_rating ?? event?.maxRating ?? event?.maxTeamRating ?? maxRating
        const validTeamsArr = eventTeams.filter((t) => isValidTeam(t, Number(maxTeamRating)))
        const invalidTeamsArr = eventTeams.filter((t) => !isValidTeam(t, Number(maxTeamRating)))
        const averageRating = validTeamsArr.length > 0
          ? Number((validTeamsArr.reduce((sum: number, t: any) => sum + (Number(t.team_rating ?? t.rating ?? 0)), 0) / validTeamsArr.length).toFixed(2))
          : 0

        const validTeamsCount = validTeamsArr.length
        const invalidTeamsCount = invalidTeamsArr.length
        const exclusionsCount = exclusions?.length ?? 0

        

  const handleOpenEditModal = (team: any) => {
    if (isLocked) {
      toast.error('Evento bloqueado. No puedes editar equipos.')
      return
    }
    // mapear a valores que espera el modal (ids + ratings)
    setEditingTeam({
      id: String(team.id),
      header_id: Number(team.header_id),
      heeler_id: Number(team.heeler_id),
      headerRating: Math.round((team.rating || 0) / 2),
      heelerRating: Math.round((team.rating || 0) / 2),
    })
    setIsCreateModalOpen(true)
  }

  const handleDeleteTeam = async (teamId: string) => {
    if (isLocked) {
      toast.error('Evento bloqueado. No puedes eliminar equipos.')
      return
    }
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
    const status = calculateTeamStatus(headerRating, heelerRating)

    try {
      if (editingTeam && (editingTeam as any).id) {
        // actualizar rating/status simple
        await edit({ id: Number((editingTeam as any).id), rating: teamRating })
        toast.success('Equipo actualizado')
      } else {
        await add({ header_id: Number(header_id), heeler_id: Number(heeler_id), rating: teamRating })
        toast.success('Equipo creado')
      }

      if (status === 'exceeds') {
        toast.warning(`Este equipo excede el rating máximo (${maxRating})`)
      }
    } catch (e: any) {
      toast.error(String(e?.message ?? e))
    }

    setIsCreateModalOpen(false)
    setEditingTeam(null)
  }

  const handleAutoCreateTeams = async () => {
    if (isLocked) {
      toast.error('Evento bloqueado. No puedes autogenerar equipos.')
      setIsAutoCreateModalOpen(false)
      return
    }

    try {
        // Clear existing teams
        try {
            await hardDeleteTeamsForEvent(Number(event?.id ?? 0))
            await new Promise((res) => setTimeout(res, 150))
            await (typeof (refresh) === 'function' ? refresh() : Promise.resolve())
        } catch (errHard: any) {
            console.error('[TeamsTab] hardDeleteTeamsForEvent failed', errHard)
            toast.error('No se pudo limpiar equipos previos antes de auto-crear.');
        }

        const all = ropers || []
        const headers = all.filter((r: any) => r.specialty === 'header' || r.specialty === 'both')
        const heelers = all.filter((r: any) => r.specialty === 'heeler' || r.specialty === 'both')

        const pairs: Array<{ header: any; heeler: any; teamRating: number }> = []
        const existingPairs = new Set<string>() // "headerId-heelerId"
        const maxR = Number(maxRating)

        // All against all: Every header with every heeler
        for (const h of headers) {
            const hId = Number(h.id)
            for (const cand of heelers) {
                const cId = Number(cand.id)
                
                if (hId === cId) continue
                if (existingPairs.has(`${hId}-${cId}`)) continue

                const th = Number(h.rating || 0)
                const tc = Number(cand.rating || 0)
                const total = th + tc
                
                // If max rating exists, enforce it
                if (maxR > 0 && total > maxR) {
                     continue 
                }

                pairs.push({ header: h, heeler: cand, teamRating: total })
                existingPairs.add(`${hId}-${cId}`)
            }
        }

        let created = 0
        let failed = 0
        // const newExclusions: Exclusion[] = [] // Keep track purely for logging if needed
        
        for (const p of pairs) {
          try {
            await add({ 
              header_id: Number(p.header.id), 
              heeler_id: Number(p.heeler.id), 
              rating: p.teamRating 
            }, { suppressRefresh: true }) 
            created++
          } catch (err: any) {
            failed++
          }
        }
        
        await refresh()
        
        // Notificar al padre que se actualizaron los equipos
        if (onTeamsUpdated) {
          onTeamsUpdated()
        }
        
        if (created > 0) {
           toast.success(`⚡ ${created} equipos (Todos vs Todos) creados.`, {
             description: failed > 0 ? `${failed} fallidos.` : undefined,
           })
        } else {
           toast.info('No se generaron nuevos equipos.')
        }

      } catch (err: any) {
        toast.error(String(err?.message ?? err))
      } finally {
        setIsAutoCreateModalOpen(false)
      }
  }

  const statusBadge = (status: Team['status']) => {
    // soporta estados antiguos (valid/exceeds/incomplete) y estados backend (active/inactive)
    if (status === 'valid' || (status as any) === 'active') {
      return (
        <Badge className="bg-emerald-50 text-emerald-700 border-emerald-200">
          <CheckCircle className="size-3 mr-1" />
          Válido
        </Badge>
      )
    }
    if (status === 'exceeds') {
      return (
        <Badge className="bg-red-50 text-red-700 border-red-200">
          <AlertTriangle className="size-3 mr-1" />
          Excede
        </Badge>
      )
    }
    if (status === 'incomplete') {
      return (
        <Badge className="bg-muted text-muted-foreground border-border">
          <XCircle className="size-3 mr-1" />
          Incompleto
        </Badge>
      )
    }
    // fallback para 'inactive' u otros
    return (
      <Badge className="bg-muted text-muted-foreground border-border">{String(status)}</Badge>
    )
  }

  const ratingBadgeColor = (rating: number) => {
    if (rating <= 3) return 'bg-blue-50 text-blue-700 border-blue-200'
    if (rating <= 5) return 'bg-emerald-50 text-emerald-700 border-emerald-200'
    return 'bg-amber-50 text-amber-700 border-amber-200'
  }

  try {
    // eslint-disable-next-line no-console
    console.debug('[TeamsTab] render', { eventId: eventIdStr, teamsLength: (teams || []).length, filteredLength: filteredTeams.length, sample: (teams || []).slice(0,3) })
  } catch (e) {}

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex justify-between items-start">
        <div>
          <h2 className="text-foreground mb-2">Equipos del Evento</h2>
          <p className="text-muted-foreground">
            Gestiona equipos registrados, crea nuevos pares o genera combinaciones automáticas.
          </p>
        </div>
        <div className="flex gap-3">
          <Button
            onClick={() => setIsAutoCreateModalOpen(true)}
            variant="outline"
            disabled={isLocked}
            className="border-border text-foreground hover:bg-background rounded-xl h-11"
          >
            <Zap className="size-4 mr-2" />
            Auto-crear
          </Button>
          <Button
            onClick={handleOpenCreateModal}
            disabled={isLocked}
            className="bg-primary hover:opacity-90 text-primary-foreground rounded-xl shadow-sm h-11"
          >
            <Plus className="size-4 mr-2" />
            Crear Equipo
          </Button>
        </div>
      </div>

      {/* Resumen */}
      <div className="bg-card rounded-xl border border-border shadow-sm p-6">
        <div className="grid grid-cols-2 md:grid-cols-6 gap-6">
          <div>
            <p className="text-muted-foreground mb-1">Evento</p>
            <p className="text-foreground">{event?.name ?? '-'}</p>
          </div>
          <div>
            <p className="text-muted-foreground mb-1">Fecha</p>
            <p className="text-foreground">
              {event?.date ? new Date(event.date).toLocaleDateString('es-ES') : '-'}
            </p>
          </div>
          <div>
            <p className="text-muted-foreground mb-1">Total Equipos</p>
            <p className="text-2xl text-foreground">{filteredTeams.length}</p>
          </div>
          <div>
            <p className="text-muted-foreground mb-1">Ropers</p>
            <p className="text-2xl text-foreground">{ropers.length}</p>
          </div>
          <div>
            <p className="text-muted-foreground mb-1">Máx Rating</p>
            <p className="text-2xl text-primary">{maxRating > 0 ? maxRating : '-'}</p>
          </div>
          <div>
            <p className="text-muted-foreground mb-1">Estado</p>
            <p className="text-foreground">
              {isLocked ? '🔒 Bloqueado' : '✓ Editable'}
            </p>
          </div>
        </div>
      </div>

      {/* Filtros */}
      <div className="grid grid-cols-1 md:grid-cols-4 gap-4">
        <div className="md:col-span-2">
            <div className="relative">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
            <Input
              placeholder="Buscar por nombre…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
                className="pl-10 bg-muted border-border rounded-xl h-11"
            />
          </div>
        </div>

        <Select value={statusFilter} onValueChange={(v: any) => setStatusFilter(v)}>
          <SelectTrigger className="bg-card border-border rounded-xl h-11">
            <SelectValue placeholder="Filtrar por estado" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">Todos</SelectItem>
            <SelectItem value="valid">Válidos</SelectItem>
            <SelectItem value="exceeds">Excede rating</SelectItem>
            <SelectItem value="incomplete">Incompletos</SelectItem>
          </SelectContent>
        </Select>

        <Select value={sortBy} onValueChange={(v: any) => setSortBy(v)}>
          <SelectTrigger className="bg-card border-border rounded-xl h-11">
            <SelectValue placeholder="Ordenar por" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="teamId">ID de Equipo</SelectItem>
            <SelectItem value="rating">Rating</SelectItem>
            <SelectItem value="header">Nombre (Header)</SelectItem>
          </SelectContent>
        </Select>
      </div>

      {/* Métricas */}
      <div className="grid grid-cols-1 md:grid-cols-4 gap-4">
        <div className="bg-emerald-50 rounded-xl p-4 border border-emerald-200 shadow-sm">
          <div className="flex items-center gap-2 mb-1">
            <CheckCircle className="size-4 text-emerald-600" />
            <p className="text-emerald-700">Equipos válidos</p>
          </div>
          <p className="text-3xl text-emerald-900">{validTeamsCount}</p>
        </div>

        <div className="bg-red-50 rounded-xl p-4 border border-red-200 shadow-sm">
          <div className="flex items-center gap-2 mb-1">
            <AlertTriangle className="size-4 text-red-600" />
            <p className="text-red-700">Equipos inválidos</p>
          </div>
          <p className="text-3xl text-red-900">{invalidTeamsCount}</p>
        </div>

        <div className="bg-blue-50 rounded-xl p-4 border border-blue-200 shadow-sm">
          <p className="text-blue-700 mb-1">Promedio Rating</p>
          <p className="text-3xl text-blue-900">{averageRating}</p>
        </div>

        <div className="bg-accent rounded-xl p-4 border border-accent shadow-sm">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-primary mb-1">Exclusiones</p>
              <p className="text-3xl text-foreground">{exclusionsCount}</p>
            </div>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setIsExclusionsModalOpen(true)}
              className="text-primary hover:bg-accent rounded-xl h-8"
            >
              Ver detalle
            </Button>
          </div>
        </div>
      </div>

      {/* Tabla de equipos */}
  <div className="bg-card rounded-xl border border-border shadow-sm overflow-hidden">
        <Table>
          <TableHeader>
            <TableRow className="bg-muted hover:bg-muted">
              <TableHead className="text-foreground">ID</TableHead>
              <TableHead className="text-foreground">Header</TableHead>
              <TableHead className="text-foreground">Heeler</TableHead>
              <TableHead className="text-foreground">Rating Header</TableHead>
              <TableHead className="text-foreground">Rating Heeler</TableHead>
              <TableHead className="text-foreground">Team Rating</TableHead>
              <TableHead className="text-foreground">Estado</TableHead>
              <TableHead className="text-foreground text-right">Acciones</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {filteredTeams.length === 0 ? (
              <TableRow>
                <TableCell colSpan={8} className="text-center py-12">
                  <p className="text-muted-foreground mb-3">No hay equipos registrados para este evento</p>
                  {!isLocked && (
                    <Button onClick={handleOpenCreateModal} variant="outline" className="border-border">
                      <Plus className="size-4 mr-2" />
                      Agregar primer equipo
                    </Button>
                  )}
                </TableCell>
              </TableRow>
            ) : (
              filteredTeams.map((team: any) => (
                <TableRow key={team.id} className="hover:bg-accent/30">
                  <TableCell>
                    <Badge variant="outline" className="border-border">
                      {String(team.id).padStart(2, '0')}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-foreground">
                    {team.headerName || String(team.header_id)}
                    {(() => {
                         const r = ropers.find(x => String(x.id) === String(team.header_id))
                         return r?.level ? ` — ${r.level}` : ''
                    })()}
                  </TableCell>
                  <TableCell className="text-foreground">
                    {team.heelerName || String(team.heeler_id)}
                    {(() => {
                         const r = ropers.find(x => String(x.id) === String(team.heeler_id))
                         return r?.level ? ` — ${r.level}` : ''
                    })()}
                  </TableCell>
                  <TableCell>
                    {(() => {
                      const headerR = ropers.find((x) => String(x.id) === String(team.header_id))
                      const hr = headerR ? Number(headerR.rating ?? 0) : null
                      return (
                        <Badge className={ratingBadgeColor(hr ?? 0)}>{hr !== null && hr !== undefined && hr >= 0 ? String(hr) : '-'}</Badge>
                      )
                    })()}
                  </TableCell>
                  <TableCell>
                    {(() => {
                      const heelerR = ropers.find((x) => String(x.id) === String(team.heeler_id))
                      const he = heelerR ? Number(heelerR.rating ?? 0) : null
                      return (
                        <Badge className={ratingBadgeColor(he ?? 0)}>{he !== null && he !== undefined && he >= 0 ? String(he) : '-'}</Badge>
                      )
                    })()}
                  </TableCell>
                  <TableCell>
                    <Badge
                      className={
                        (team.status === 'valid' || team.status === 'active')
                          ? 'bg-emerald-50 text-emerald-700 border-emerald-200'
                          : team.status === 'exceeds'
                          ? 'bg-red-50 text-red-700 border-red-200'
                          : 'bg-muted text-muted-foreground border-border'
                      }
                    >
                      {team.rating ?? '-'}
                    </Badge>
                  </TableCell>
                  <TableCell>{statusBadge(team.status)}</TableCell>
                  <TableCell className="text-right">
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <Button variant="ghost" size="sm" disabled={isLocked} className="hover:bg-accent">
                          <MoreVertical className="size-4" />
                        </Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        <DropdownMenuItem onClick={() => handleOpenEditModal(team)}>
                          <Edit className="size-4 mr-2" />
                          Editar
                        </DropdownMenuItem>
                        <DropdownMenuItem>
                          <Eye className="size-4 mr-2" />
                          Ver detalles
                        </DropdownMenuItem>
                        <DropdownMenuSeparator />
                        <DropdownMenuItem onClick={() => handleDeleteTeam(team.id)} className="text-red-600">
                          <Trash2 className="size-4 mr-2" />
                          Eliminar
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
        </Table>
      </div>

      {/* Modal Crear/Editar */}
      <AddTeamModal
        isOpen={isCreateModalOpen}
        onClose={() => { setIsCreateModalOpen(false); setEditingTeam(null) }}
  roperOptions={ropers.map((r) => ({ id: r.id, label: `${r.firstName} ${r.lastName} — ${r.level ?? 'amateur'}`, rating: r.rating }))}
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

      {/* Modal Auto-crear */}
      <Dialog open={isAutoCreateModalOpen} onOpenChange={setIsAutoCreateModalOpen}>
        <DialogContent className="sm:max-w-[500px]">
          <DialogHeader>
            <DialogTitle className="text-foreground flex items-center gap-2">
              <Zap className="size-5 text-primary" />
              Auto-crear equipos
            </DialogTitle>
            <DialogDescription>Generación de equipos "Todos contra Todos"</DialogDescription>
          </DialogHeader>

          <div className="space-y-4 py-4">
            <div className="bg-accent rounded-xl p-4 border border-accent shadow-sm">
              <p className="text-sm text-foreground">
                Se combinará cada <strong>Header</strong> con cada <strong>Heeler</strong> disponible.
              </p>
              <ul className="list-disc list-inside mt-2 text-sm text-muted-foreground">
                <li>Se respetará el límite de rating máximo si existe.</li>
                <li>No se repetirán parejas ya existentes.</li>
                <li>No se crearán equipos donde header y heeler sean la misma persona.</li>
              </ul>
            </div>
            
            <div className="bg-blue-50 text-blue-800 rounded-xl p-4 border border-blue-200 shadow-sm">
                 <p className="text-sm">
                    <strong>Nota:</strong> Esta acción borrará todos los equipos actuales para garantizar una generación limpia.
                 </p>
            </div>
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setIsAutoCreateModalOpen(false)} className="border-border">
              Cancelar
            </Button>
            <Button onClick={handleAutoCreateTeams} className="bg-primary text-primary-foreground">
              <Zap className="size-4 mr-2" />
              Generar Combinaciones
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Modal Exclusiones */}
      <Dialog open={isExclusionsModalOpen} onOpenChange={setIsExclusionsModalOpen}>
        <DialogContent className="sm:max-w-[600px]">
          <DialogHeader>
            <DialogTitle className="text-foreground">Exclusiones de autobalance</DialogTitle>
            <DialogDescription>Ropers excluidos durante la generación automática</DialogDescription>
          </DialogHeader>

          <div className="py-4">
            <Table>
              <TableHeader>
                <TableRow className="bg-muted">
                  <TableHead className="text-foreground">Roper</TableHead>
                  <TableHead className="text-foreground">Motivo</TableHead>
                  <TableHead className="text-foreground">Fecha</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {exclusions.length === 0 ? (
                  <TableRow>
                    <TableCell colSpan={3} className="text-center py-8 text-muted-foreground">
                      No hay exclusiones registradas
                    </TableCell>
                  </TableRow>
                ) : (
                  exclusions.map((ex) => (
                    <TableRow key={ex.id}>
                      <TableCell className="text-foreground">{ex.roperName}</TableCell>
                      <TableCell className="text-muted-foreground">{ex.reason}</TableCell>
                      <TableCell className="text-muted-foreground">
                        {new Date(ex.date).toLocaleDateString('es-ES')}
                      </TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setIsExclusionsModalOpen(false)} className="border-border">
              Cerrar
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
