import { useEffect, useMemo, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from './ui/dialog'
import { Button } from './ui/button'
import { Input } from './ui/input'
import { Label } from './ui/label'
import { Switch } from './ui/switch'
import { Select, SelectTrigger, SelectContent, SelectItem, SelectValue } from './ui/select'
import { toast } from 'sonner'
import type {
  BackupInspection,
  Event as EventType,
  ImportEventBackupResult,
  Series,
} from '../types'
import { getSeries, importEventBackup, inspectEventBackup } from '../lib/api'
import {
  buildImportSuccessDescription,
  canConfirmBackupImport,
  getBackupPreviewRows,
  getEventBackupErrorMessage,
} from '../lib/event-backup-ui'

type ModalMode = 'create' | 'import'
type RestoreStatusMode = 'preserve' | 'force_upcoming' | 'force_locked'

export function NewEventModal({
  isOpen,
  onClose,
  onCreateEvent,
  onUpdateEvent,
  onImportEvent,
  initialEvent,
  seriesId,
}: {
  isOpen: boolean
  onClose: () => void
  onCreateEvent?: (e: EventType) => void | Promise<void>
  onUpdateEvent?: (id: string, patch: any) => void | Promise<void>
  onImportEvent?: (result: ImportEventBackupResult, targetSeriesId: number) => void | Promise<void>
  initialEvent?: EventType | null
  seriesId: string | number
}) {
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
  const [mode, setMode] = useState<ModalMode>('create')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const [seriesOptions, setSeriesOptions] = useState<Series[]>([])
  const [seriesLoading, setSeriesLoading] = useState(false)
  const [importFilePath, setImportFilePath] = useState('')
  const [inspection, setInspection] = useState<BackupInspection | null>(null)
  const [importError, setImportError] = useState<string | null>(null)
  const [inspectLoading, setInspectLoading] = useState(false)
  const [importLoading, setImportLoading] = useState(false)
  const [targetSeriesId, setTargetSeriesId] = useState<string>('')
  const [restoreStatusMode, setRestoreStatusMode] = useState<RestoreStatusMode>('preserve')
  const [dedupeRopers, setDedupeRopers] = useState(true)

  const hasFixedSeries = useMemo(() => {
    const parsed = Number(seriesId)
    return Number.isFinite(parsed) && parsed > 0
  }, [seriesId])

  useEffect(() => {
    if (!isOpen) {
      setName('')
      setDate(new Date().toISOString().slice(0, 10))
      setRounds(3)
      setEntryFee('')
      setMaxTeamRating('')
      setIsMaxRatingEnabled(false)
      setPayoffAllocation('')
      setAdminPin('')
      setStatus('draft')
      setMode('create')
      setError(null)
      setLoading(false)
      setIsEditMode(false)
      setImportFilePath('')
      setInspection(null)
      setImportError(null)
      setInspectLoading(false)
      setImportLoading(false)
      setRestoreStatusMode('preserve')
      setDedupeRopers(true)
      setTargetSeriesId(hasFixedSeries ? String(seriesId) : '')
      return
    }

    if (isOpen && initialEvent) {
      setIsEditMode(true)
      setMode('create')
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
    } else if (isOpen) {
      setIsEditMode(false)
      setMode('create')
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen, seriesId, initialEvent])

  useEffect(() => {
    if (!isOpen || isEditMode) return

    let mounted = true
    setSeriesLoading(true)
    getSeries()
      .then((rows) => {
        if (!mounted) return
        setSeriesOptions(rows as Series[])
        const fallbackSeriesId =
          hasFixedSeries
            ? String(seriesId)
            : rows.length > 0
              ? String(rows[0].id)
              : ''
        setTargetSeriesId((current) => current || fallbackSeriesId)
      })
      .catch(() => {
        if (!mounted) return
        setSeriesOptions([])
      })
      .finally(() => {
        if (mounted) setSeriesLoading(false)
      })

    return () => {
      mounted = false
    }
  }, [hasFixedSeries, isEditMode, isOpen, seriesId])

  const previewRows = useMemo(
    () => (inspection ? getBackupPreviewRows(inspection) : []),
    [inspection]
  )

  const selectedTargetSeries = useMemo(
    () => seriesOptions.find((item) => String(item.id) === targetSeriesId) ?? null,
    [seriesOptions, targetSeriesId]
  )

  const canConfirmImport = canConfirmBackupImport({
    filePath: importFilePath,
    inspection,
    targetSeriesId: targetSeriesId ? Number(targetSeriesId) : null,
    isImporting: importLoading,
  })

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
    if (!Number.isFinite(Number(rounds)) || Number(rounds) < 1) {
      setError('Rondas debe ser un número entero mayor o igual a 1.')
      return false
    }
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
        const patch: Record<string, unknown> = {}
        const assignIfChanged = (key: string, nextValue: unknown, prevValue: unknown) => {
          const prev = prevValue ?? null
          const next = nextValue ?? null
          if (prev !== next) {
            patch[key] = next
          }
        }

        assignIfChanged('name', newEvent.name, initialEvent.name)
        assignIfChanged('date', newEvent.date, initialEvent.date)
        assignIfChanged('rounds', newEvent.rounds, initialEvent.rounds)
        assignIfChanged('status', newEvent.status, initialEvent.status)

        const normalizedEntryFee = entryFee.trim() === '' ? null : Number(entryFee)
        assignIfChanged('entry_fee', normalizedEntryFee, initialEvent.entryFee ?? null)

        const normalizedMaxRating =
          isMaxRatingEnabled && maxTeamRating.trim() !== '' ? Number(maxTeamRating) : null
        assignIfChanged('max_team_rating', normalizedMaxRating, initialEvent.maxTeamRating ?? null)

        const normalizedPayoff = payoffAllocation.trim() === '' ? null : payoffAllocation.trim()
        assignIfChanged('payoff_allocation', normalizedPayoff, initialEvent.payoffAllocation ?? null)

        const normalizedAdminPin = adminPin.trim() === '' ? null : adminPin.trim()
        assignIfChanged('admin_pin', normalizedAdminPin, initialEvent.adminPin ?? null)

        if (Object.keys(patch).length > 0) {
          await onUpdateEvent?.(String(initialEvent.id), patch)
        } else {
          toast.success('Sin cambios por guardar')
        }
      } else {
        await onCreateEvent?.(newEvent)
      }
      onClose()
    } catch (err: any) {
      setError(err?.toString?.() ?? 'Error creando el evento')
    } finally {
      setLoading(false)
    }
  }

  const handleSelectBackupFile = async () => {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: 'Backup XLSX', extensions: ['xlsx'] }],
    })

    const filePath = Array.isArray(selected) ? selected[0] : selected
    if (!filePath) return

    setImportFilePath(filePath)
    setImportError(null)
    setInspection(null)
    setInspectLoading(true)

    try {
      const result = await inspectEventBackup(filePath)
      setInspection(result)
      if (!targetSeriesId && hasFixedSeries) {
        setTargetSeriesId(String(seriesId))
      }
    } catch (err) {
      const message = getEventBackupErrorMessage(err, 'No se pudo leer el backup seleccionado.')
      setImportError(message)
      toast.error('Backup inválido', { description: message })
    } finally {
      setInspectLoading(false)
    }
  }

  const handleImportBackup = async () => {
    if (!canConfirmImport) {
      setImportError('Selecciona un backup válido y una serie destino antes de continuar.')
      return
    }

    setImportLoading(true)
    setImportError(null)

    try {
      const result = await importEventBackup({
        filePath: importFilePath,
        targetSeriesId: Number(targetSeriesId),
        restoreStatusMode,
        dedupeRopers,
      })

      await onImportEvent?.(result, Number(targetSeriesId))

      toast.success('Evento importado', {
        description: buildImportSuccessDescription(result, selectedTargetSeries?.name),
      })

      if (result.warnings.length > 0) {
        toast.warning('Importación completada con advertencias', {
          description: result.warnings.join(' | '),
        })
      }

      onClose()
    } catch (err) {
      const message = getEventBackupErrorMessage(err, 'No se pudo importar el backup.')
      setImportError(message)
      toast.error('No se pudo importar el backup', { description: message })
    } finally {
      setImportLoading(false)
    }
  }

  if (!isOpen) return null

  return (
    <Dialog open={isOpen} onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="sm:max-w-[720px] max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="text-foreground">
            {isEditMode ? 'Editar evento' : mode === 'import' ? 'Importar evento desde backup' : 'Crear nuevo evento'}
          </DialogTitle>
          <DialogDescription>
            {isEditMode
              ? 'Modifica los campos del evento y presiona Guardar evento para aplicar los cambios.'
              : mode === 'import'
                ? 'Selecciona un backup XLSX, revisa el preview y restaura el evento como uno nuevo.'
                : 'Completa los datos para crear un nuevo evento dentro de la serie seleccionada.'}
          </DialogDescription>
        </DialogHeader>

        {!isEditMode && (
          <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
            <Button
              type="button"
              variant={mode === 'create' ? 'default' : 'outline'}
              onClick={() => setMode('create')}
              disabled={loading || importLoading}
            >
              Crear evento
            </Button>
            <Button
              type="button"
              variant={mode === 'import' ? 'default' : 'outline'}
              onClick={() => setMode('import')}
              disabled={loading || importLoading}
            >
              Importar evento desde backup
            </Button>
          </div>
        )}

        {mode === 'import' && !isEditMode ? (
          <div className="space-y-4">
            {importError && (
              <div className="bg-red-50 text-red-700 border border-red-100 rounded p-3">
                {importError}
              </div>
            )}

            <div className="space-y-2">
              <Label htmlFor="backup-file">Archivo de backup (.xlsx)</Label>
              <div className="flex gap-2">
                <Input
                  id="backup-file"
                  value={importFilePath}
                  readOnly
                  placeholder="Selecciona un backup de evento"
                  disabled={inspectLoading || importLoading}
                />
                <Button
                  type="button"
                  variant="outline"
                  onClick={handleSelectBackupFile}
                  disabled={inspectLoading || importLoading}
                >
                  {inspectLoading ? 'Leyendo...' : 'Seleccionar'}
                </Button>
              </div>
            </div>

            {inspection && (
              <div className="rounded-lg border border-border bg-muted/30 p-4 space-y-4">
                <div>
                  <div className="text-sm font-medium text-foreground">Preview del backup</div>
                  <div className="text-xs text-muted-foreground">{inspection.format}</div>
                </div>

                <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                  {previewRows.map((row) => (
                    <div key={row.label} className="rounded-md border border-border bg-background px-3 py-2">
                      <div className="text-xs uppercase tracking-wide text-muted-foreground">{row.label}</div>
                      <div className="text-sm font-medium text-foreground">{row.value}</div>
                    </div>
                  ))}
                </div>

                {inspection.warnings.length > 0 && (
                  <div className="rounded-md border border-amber-200 bg-amber-50 px-3 py-3">
                    <div className="text-sm font-medium text-amber-900">Warnings</div>
                    <ul className="mt-2 list-disc pl-5 text-sm text-amber-800 space-y-1">
                      {inspection.warnings.map((warning, index) => (
                        <li key={`${warning}-${index}`}>{warning}</li>
                      ))}
                    </ul>
                  </div>
                )}
              </div>
            )}

            <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
              <div className="space-y-2">
                <Label htmlFor="backup-target-series">Serie destino</Label>
                <Select
                  value={targetSeriesId}
                  onValueChange={setTargetSeriesId}
                  disabled={seriesLoading || importLoading}
                >
                  <SelectTrigger id="backup-target-series" className="w-full">
                    <SelectValue placeholder={seriesLoading ? 'Cargando series...' : 'Selecciona serie'} />
                  </SelectTrigger>
                  <SelectContent>
                    {seriesOptions.map((option) => (
                      <SelectItem key={option.id} value={String(option.id)}>
                        {option.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              <div className="space-y-2">
                <Label htmlFor="backup-status-mode">Estado al restaurar</Label>
                <Select
                  value={restoreStatusMode}
                  onValueChange={(value: RestoreStatusMode) => setRestoreStatusMode(value)}
                  disabled={importLoading}
                >
                  <SelectTrigger id="backup-status-mode" className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="preserve">preserve</SelectItem>
                    <SelectItem value="force_upcoming">force_upcoming</SelectItem>
                    <SelectItem value="force_locked">force_locked</SelectItem>
                  </SelectContent>
                </Select>
              </div>
            </div>

            <div className="rounded-lg border border-border bg-background px-4 py-3">
              <div className="flex items-center justify-between gap-4">
                <div>
                  <div className="text-sm font-medium text-foreground">Reusar ropers existentes</div>
                  <div className="text-xs text-muted-foreground">
                    Match exacto por nombre, apellido, specialty y rating.
                  </div>
                </div>
                <Switch
                  checked={dedupeRopers}
                  onCheckedChange={(checked) => setDedupeRopers(Boolean(checked))}
                  disabled={importLoading}
                />
              </div>
            </div>

            <DialogFooter>
              <div className="flex items-center justify-end gap-2 w-full">
                <Button variant="outline" onClick={onClose} disabled={importLoading || inspectLoading}>
                  Cancelar
                </Button>
                <Button
                  type="button"
                  onClick={handleImportBackup}
                  className="bg-primary text-primary-foreground"
                  aria-busy={importLoading}
                  disabled={!canConfirmImport || importLoading || inspectLoading}
                >
                  {importLoading ? 'Importando...' : 'Importar evento'}
                </Button>
              </div>
            </DialogFooter>
          </div>
        ) : (
          <form onSubmit={handleSubmit} className="space-y-4">
            {error && (
              <div className="bg-red-50 text-red-700 border border-red-100 rounded p-3">
                {error}
              </div>
            )}

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

            <div>
              <Label htmlFor="event-adminPin">PIN de Administrador (4 d&iacute;gitos)</Label>
              <Input
                id="event-adminPin"
                value={adminPin}
                onChange={(e) => {
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
                <Button type="button" variant="outline" onClick={() => { onClose(); }} disabled={loading}>
                  Cancelar
                </Button>
                <Button type="submit" onClick={(e) => handleSubmit(e)} className="bg-primary text-primary-foreground" aria-busy={loading} disabled={loading || !seriesId}>
                  {isEditMode ? 'Guardar evento' : 'Crear evento'}
                </Button>
              </div>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  )
}
