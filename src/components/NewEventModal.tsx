import { useEffect, useState } from 'react'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from './ui/dialog'
import { Button } from './ui/button'
import { Input } from './ui/input'
import { Label } from './ui/label'
import { Switch } from './ui/switch'
import { Select, SelectTrigger, SelectContent, SelectItem, SelectValue } from './ui/select'
import type { Event as EventType } from '../types'

export function NewEventModal({
  isOpen,
  onClose,
  onCreateEvent,
  onUpdateEvent,
  initialEvent,
  seriesId,
}: {
  isOpen: boolean
  onClose: () => void
  onCreateEvent?: (e: EventType) => void
  onUpdateEvent?: (id: string, patch: any) => void
  initialEvent?: EventType | null
  seriesId: string | number
}) {
  // seriesId is provided by the parent (the open series). No selector in the modal.
  const [name, setName] = useState<string>('')
  const [date, setDate] = useState<string>(new Date().toISOString().slice(0, 10))
  const [rounds, setRounds] = useState<number>(3)
  const [entryFee, setEntryFee] = useState<string>('')
  const [maxTeamRating, setMaxTeamRating] = useState<string>('')
  const [isMaxRatingEnabled, setIsMaxRatingEnabled] = useState<boolean>(false)
  const [payoffAllocation, setPayoffAllocation] = useState<string>('')
  const [adminPin, setAdminPin] = useState<string>('')
  const [status, setStatus] = useState<EventType['status']>('draft')
  const [isEditMode, setIsEditMode] = useState<boolean>(false)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!isOpen) {
      // reset form
      setName('')
      setDate(new Date().toISOString().slice(0, 10))
      setRounds(3)
      setEntryFee('')
      setMaxTeamRating('')
      setIsMaxRatingEnabled(false)
      setPayoffAllocation('')
      setAdminPin('')
      setStatus('draft')
      setError(null)
      setLoading(false)
      setIsEditMode(false)
    }
    // if opening and there's an initialEvent, populate fields
    if (isOpen && initialEvent) {
      setIsEditMode(true)
      setName(initialEvent.name ?? '')
      setDate(initialEvent.date ?? new Date().toISOString().slice(0, 10))
      setRounds(initialEvent.rounds ?? 3)
      setEntryFee(initialEvent.entryFee ? String(initialEvent.entryFee) : '')
      if (initialEvent.maxTeamRating !== undefined && initialEvent.maxTeamRating !== null) {
        setMaxTeamRating(String(initialEvent.maxTeamRating))
        setIsMaxRatingEnabled(true)
      } else {
        setMaxTeamRating('')
        setIsMaxRatingEnabled(false)
      }
      setPayoffAllocation(initialEvent.payoffAllocation ?? '')
      setAdminPin(initialEvent.adminPin ?? '')
      setStatus((initialEvent.status as EventType['status']) ?? 'draft')
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen, seriesId, initialEvent])

  const validate = () => {
    setError(null)
    if (!seriesId) {
      setError('No hay una serie activa seleccionada. Abre una serie antes de crear un evento.')
      return false
    }
    if (!name.trim()) {
      setError('El nombre del evento es requerido.')
      return false
    }
    if (!date) {
      setError('La fecha del evento es requerida.')
      return false
    }
    // rounds
    if (!Number.isFinite(Number(rounds)) || Number(rounds) < 1) {
      setError('Rondas debe ser un número entero mayor o igual a 1.')
      return false
    }
    // entryFee
    if (entryFee) {
      const v = Number(entryFee)
      if (Number.isNaN(v) || v < 0) {
        setError('Entry fee debe ser un número mayor o igual a 0.')
        return false
      }
    }
    if (maxTeamRating) {
      const v = Number(maxTeamRating)
      if (Number.isNaN(v) || v < 0) {
        setError('Max team rating debe ser un número mayor o igual a 0.')
        return false
      }
    }
    // adminPin
    if (adminPin && !/^\d{4}$/.test(adminPin)) {
      setError('El PIN de administrador debe ser de 4 dígitos numéricos.')
      return false
    }

    return true
  }

  const handleSubmit = async (e?: React.FormEvent) => {
    e?.preventDefault()
    if (!validate()) return
    setLoading(true)
    try {
      const newEvent: EventType = {
        id: isEditMode && initialEvent ? initialEvent.id : 0,
        seriesId: Number(seriesId),
        name: name.trim(),
        date: date,
        status: (status as any) ?? 'draft',
        teamsCount: 0,
        rounds: Number(rounds),
        entryFee: entryFee ? Number(entryFee) : undefined,
        maxTeamRating: (isMaxRatingEnabled && maxTeamRating) ? Number(maxTeamRating) : undefined,
        pot: 0,
        payoffAllocation: payoffAllocation || undefined,
        adminPin: adminPin || undefined,
      }

      if (isEditMode && initialEvent) {
        // build patch
        const patch: any = {
          name: newEvent.name,
          date: newEvent.date,
          rounds: newEvent.rounds,
          status: newEvent.status,
          entry_fee: newEvent.entryFee ?? null,
          max_team_rating: newEvent.maxTeamRating ?? null,
          payoff_allocation: newEvent.payoffAllocation ?? null,
          admin_pin: newEvent.adminPin ?? null,
        }
        onUpdateEvent?.(String(initialEvent.id), patch)
      } else {
        onCreateEvent?.(newEvent)
      }
      onClose()
    } catch (err: any) {
      setError(err?.toString?.() ?? 'Error creando el evento')
    } finally {
      setLoading(false)
    }
  }

  if (!isOpen) return null

  return (
    <Dialog open={isOpen} onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="sm:max-w-[600px]">
        <DialogHeader>
          <DialogTitle className="text-foreground">{isEditMode ? 'Editar evento' : 'Crear nuevo evento'}</DialogTitle>
          <DialogDescription>
            {isEditMode
              ? 'Modifica los campos del evento y presiona Guardar evento para aplicar los cambios.'
              : 'Completa los datos para crear un nuevo evento dentro de la serie seleccionada.'}
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={handleSubmit} className="space-y-4">
          {error && (
            <div className="bg-red-50 text-red-700 border border-red-100 rounded p-3">
              {error}
            </div>
          )}

          {/* Series selector removed: event will be associated to the open series (passed via seriesId prop) */}

          <div>
            <Label htmlFor="event-name">Nombre</Label>
            <Input id="event-name" value={name} onChange={(e) => setName(e.target.value)} disabled={loading} aria-invalid={!!error && !name.trim()} />
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div>
              <Label htmlFor="event-date">Fecha</Label>
              <Input id="event-date" type="date" value={date} onChange={(e) => setDate(e.target.value)} disabled={loading} aria-invalid={!!error && !date} />
            </div>
            <div>
              <Label htmlFor="event-rounds">Rondas</Label>
              <Input id="event-rounds" type="number" min={1} value={String(rounds)} onChange={(e) => setRounds(Number(e.target.value))} disabled={loading} aria-invalid={!!error && Number(rounds) < 1} />
            </div>
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div className="flex flex-col gap-2">
              <div className="flex items-center h-8">
                <Label htmlFor="event-entryFee">Entry fee</Label>
              </div>
              <Input id="event-entryFee" type="number" min={0} value={entryFee} onChange={(e) => setEntryFee(e.target.value)} disabled={loading} aria-invalid={!!error && entryFee !== '' && Number(entryFee) < 0} />
            </div>
            <div className="flex flex-col gap-2">
              <div className="flex items-center justify-between h-8">
                <Label htmlFor="event-maxTeamRating" className={!isMaxRatingEnabled ? "text-muted-foreground" : ""}>Max team rating</Label>
                <Switch
                  checked={isMaxRatingEnabled}
                  onCheckedChange={(c) => {
                    setIsMaxRatingEnabled(c)
                    if (!c) setMaxTeamRating('')
                  }}
                  disabled={loading}
                />
              </div>
              <Input
                id="event-maxTeamRating"
                type="number"
                min={0}
                value={maxTeamRating}
                onChange={(e) => setMaxTeamRating(e.target.value)}
                disabled={loading || !isMaxRatingEnabled}
                className={!isMaxRatingEnabled ? "opacity-50" : ""}
                placeholder={!isMaxRatingEnabled ? "Sin límite" : "Ej. 5.5"}
                aria-invalid={!!error && maxTeamRating !== '' && Number(maxTeamRating) < 0}
              />
            </div>
          </div>

          {/* Payoff Allocation removed as per request */}
          
          <div>
             <Label htmlFor="event-adminPin">PIN de Administrador (4 d&iacute;gitos)</Label>
             <Input 
               id="event-adminPin" 
               value={adminPin} 
               onChange={(e) => {
                 // Allow only numbers and max 4 chars
                 const val = e.target.value.replace(/\D/g, '').slice(0, 4)
                 setAdminPin(val)
               }} 
               placeholder="####"
               disabled={loading} 
             />
             <p className="text-xs text-muted-foreground mt-1">Requerido para acciones sensibles (borrar, revertir, etc).</p>
          </div>

          <div>
            <Label htmlFor="event-status">Estado</Label>
            <Select value={status} onValueChange={(v: any) => setStatus(v)}>
              <SelectTrigger id="event-status" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="draft">draft</SelectItem>
                <SelectItem value="active">active</SelectItem>
                <SelectItem value="locked">locked</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <DialogFooter>
            <div className="flex items-center justify-end gap-2 w-full">
              <Button variant="outline" onClick={() => { onClose(); }} disabled={loading}>
                Cancelar
              </Button>
              <Button type="submit" onClick={(e) => handleSubmit(e)} className="bg-primary text-primary-foreground" aria-busy={loading} disabled={loading || !seriesId}>
                {isEditMode ? 'Guardar evento' : 'Crear evento'}
              </Button>
            </div>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

