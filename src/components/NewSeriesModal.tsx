import { useEffect, useState } from 'react'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from './ui/dialog'
import { Button } from './ui/button'
import { Input } from './ui/input'
import { Label } from './ui/label'
import { Select, SelectTrigger, SelectContent, SelectItem, SelectValue } from './ui/select'
import type { Series } from '../types'

export function NewSeriesModal({
  isOpen,
  onClose,
  onCreateSeries,
  initialValue,
}: {
  isOpen: boolean
  onClose: () => void
  onCreateSeries: (s: Series) => void
  initialValue?: Series | null
}) {
  const [name, setName] = useState('')
  // season is derived automatically from dates; not an input field anymore
  const [status, setStatus] = useState<Series['status']>('upcoming')
  const [startDate, setStartDate] = useState<string>('')
  const [endDate, setEndDate] = useState<string>('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!isOpen) {
      // reset form when closed
      setName('')
      setStatus('upcoming')
      setStartDate('')
      setEndDate('')
      setError(null)
      setLoading(false)
    } else {
      // if editing, initialize fields
      if (initialValue) {
        setName(initialValue.name ?? '')
        setStatus(initialValue.status ?? 'upcoming')
        // parse dateRange "YYYY-MM-DD - YYYY-MM-DD" or single date
        const dr = initialValue.dateRange ?? ''
        if (dr.includes(' - ')) {
          const [s, e] = dr.split(' - ').map((p) => p.trim())
          setStartDate(s)
          setEndDate(e)
        } else if (dr) {
          setStartDate(dr)
          setEndDate('')
        } else {
          setStartDate('')
          setEndDate('')
        }
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen, initialValue])

  const validate = () => {
    setError(null)
    if (!name.trim()) {
      setError('El nombre es requerido.')
      return false
    }
    // season is derived automatically from dates; no season field validation
    if (startDate && endDate) {
      if (startDate > endDate) {
        setError('La fecha de inicio debe ser anterior o igual a la fecha de fin.')
        return false
      }
    }
    return true
  }

  const handleSubmit = async (e?: React.FormEvent) => {
    e?.preventDefault()
    if (!validate()) return
    setLoading(true)
    try {
  const id = initialValue?.id ?? Date.now().toString()
      const dateRange = startDate && endDate ? `${startDate} - ${endDate}` : startDate || endDate || ''

      const deriveSeason = (start?: string, end?: string) => {
        const nowYear = new Date().getFullYear().toString()
        if (start && end) {
          const ys = start.slice(0, 4)
          const ye = end.slice(0, 4)
          return ys === ye ? ys : `${ys}-${ye}`
        }
        if (start) return start.slice(0, 4)
        if (end) return end.slice(0, 4)
        return nowYear
      }

      const seasonValue = deriveSeason(startDate || undefined, endDate || undefined)

      const newSeries: Series = {
        id: Number(id),
        name: name.trim(),
        season: seasonValue,
        status: status as Series['status'],
        dateRange,
        eventsCount: initialValue?.eventsCount ?? 0,
        progress: initialValue?.progress ?? 0,
        description: initialValue?.description ?? '',
      }

      onCreateSeries(newSeries)
      onClose()
    } catch (err: any) {
      setError(err?.toString?.() ?? 'Error creando la serie')
    } finally {
      setLoading(false)
    }
  }

  if (!isOpen) return null

  return (
    <Dialog open={isOpen} onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="sm:max-w-[600px]">
          <DialogHeader>
            <DialogTitle className="text-foreground">{initialValue ? 'Editar serie' : 'Crear nueva serie'}</DialogTitle>
            <DialogDescription>{initialValue ? 'Modifica los campos y guarda los cambios.' : 'Completa la información para crear una nueva serie.'}</DialogDescription>
          </DialogHeader>

        <form onSubmit={handleSubmit} className="space-y-4">
          {error && (
            <div className="bg-red-50 text-red-700 border border-red-100 rounded p-3">
              {error}
            </div>
          )}

          <div>
            <Label htmlFor="series-name">Nombre</Label>
            <Input id="series-name" value={name} onChange={(e) => setName(e.target.value)} disabled={loading} />
          </div>

          {/* Temporada eliminada: se deriva automáticamente desde las fechas */}

          <div>
            <Label htmlFor="series-status">Estado</Label>
            <Select value={status} onValueChange={(v: any) => setStatus(v)}>
              <SelectTrigger id="series-status" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="upcoming">upcoming</SelectItem>
                <SelectItem value="active">active</SelectItem>
                <SelectItem value="archived">archived</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div>
              <Label htmlFor="start-date">Fecha inicio</Label>
              <Input id="start-date" type="date" value={startDate} onChange={(e) => setStartDate(e.target.value)} disabled={loading} />
            </div>
            <div>
              <Label htmlFor="end-date">Fecha fin</Label>
              <Input id="end-date" type="date" value={endDate} onChange={(e) => setEndDate(e.target.value)} disabled={loading} />
            </div>
          </div>

          <DialogFooter>
            <div className="flex items-center justify-end gap-2 w-full">
              <Button variant="outline" onClick={() => { onClose(); }} disabled={loading}>
                Cancelar
              </Button>
              <Button type="submit" onClick={(e) => handleSubmit(e)} className="bg-primary text-primary-foreground" aria-busy={loading}>
                {initialValue ? 'Guardar serie' : 'Crear serie'}
              </Button>
            </div>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
