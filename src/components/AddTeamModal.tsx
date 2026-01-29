import React, { useEffect, useState } from 'react'
import { Button } from './ui/button'
import { Input } from './ui/input'
import { Label } from './ui/label'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from './ui/dialog'

interface AddTeamModalProps {
  isOpen: boolean
  onClose: () => void
  // header_id and heeler_id now use numeric ids from ropers
  onAddTeam: (team: { header_id: number; heeler_id: number; headerRating: number; heelerRating: number }) => void
  initialValue?: { header_id: number; heeler_id: number; headerRating: number; heelerRating: number }
  // options for selects
  roperOptions?: Array<{ id: number; label: string; rating?: number }>
  maxRating?: number
}

export function AddTeamModal({ isOpen, onClose, onAddTeam, initialValue, roperOptions, maxRating }: AddTeamModalProps) {
  const [form, setForm] = useState({
    header_id: '',
    heeler_id: '',
    headerRating: '',
    heelerRating: '',
  })
  const [submitting, setSubmitting] = useState(false)

  useEffect(() => {
    if (initialValue) {
      setForm({
        header_id: String(initialValue.header_id),
        heeler_id: String(initialValue.heeler_id),
        headerRating: String(initialValue.headerRating),
        heelerRating: String(initialValue.heelerRating),
      })
    } else {
      setForm({ header_id: '', heeler_id: '', headerRating: '', heelerRating: '' })
    }
  }, [initialValue, isOpen])

  const total = (parseFloat(form.headerRating) || 0) + (parseFloat(form.heelerRating) || 0)

  // when user selects header or heeler, auto-fill rating from roperOptions if present
  useEffect(() => {
    // header
    if (form.header_id) {
      const found = roperOptions?.find((r) => String(r.id) === String(form.header_id))
      if (found && (form.headerRating === '' || Number(form.headerRating) !== Number(found.rating || 0))) {
        setForm((prev) => ({ ...prev, headerRating: String(found.rating ?? '0') }))
      }
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [form.header_id])

  useEffect(() => {
    if (form.heeler_id) {
      const found = roperOptions?.find((r) => String(r.id) === String(form.heeler_id))
      if (found && (form.heelerRating === '' || Number(form.heelerRating) !== Number(found.rating || 0))) {
        setForm((prev) => ({ ...prev, heelerRating: String(found.rating ?? '0') }))
      }
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [form.heeler_id])

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setSubmitting(true)
    await new Promise((r) => setTimeout(r, 250))
    onAddTeam({
      header_id: Number(form.header_id),
      heeler_id: Number(form.heeler_id),
      headerRating: parseFloat(form.headerRating),
      heelerRating: parseFloat(form.heelerRating),
    })
    setSubmitting(false)
  }

  const update = (field: string, value: string) => setForm((prev) => ({ ...prev, [field]: value }))

  return (
    <Dialog open={isOpen} onOpenChange={onClose}>
      <DialogContent className="sm:max-w-[520px]">
        <DialogHeader>
          <DialogTitle className="text-foreground">
            {initialValue ? 'Editar equipo' : 'Crear nuevo equipo'}
          </DialogTitle>
        </DialogHeader>

        <form onSubmit={handleSubmit} className="space-y-5 mt-2">
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="header">Header</Label>
              <select
                id="header"
                value={form.header_id}
                onChange={(e) => update('header_id', e.target.value)}
                required
                className="w-full rounded-lg border border-border bg-card px-3 py-2 text-sm"
              >
                <option value="">Selecciona Header</option>
                {roperOptions?.map((opt: { id: number; label: string }) => (
                  <option key={opt.id} value={String(opt.id)}>{opt.label}</option>
                ))}
              </select>
            </div>

            <div className="space-y-2">
              <Label htmlFor="headerRating">Header Rating</Label>
              <Input
                id="headerRating"
                type="number"
                step="1"
                min="0"
                max="10"
                value={form.headerRating}
                onChange={(e) => update('headerRating', e.target.value)}
                placeholder="4"
                required
                className="bg-muted border-border rounded-xl h-11"
              />
            </div>
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="heeler">Heeler</Label>
              <select
                id="heeler"
                value={form.heeler_id}
                onChange={(e) => update('heeler_id', e.target.value)}
                required
                className="w-full rounded-lg border border-border bg-card px-3 py-2 text-sm"
              >
                <option value="">Selecciona Heeler</option>
                {roperOptions?.map((opt: { id: number; label: string }) => (
                  <option key={opt.id} value={String(opt.id)}>{opt.label}</option>
                ))}
              </select>
            </div>

            <div className="space-y-2">
              <Label htmlFor="heelerRating">Heeler Rating</Label>
              <Input
                id="heelerRating"
                type="number"
                step="1"
                min="0"
                max="10"
                value={form.heelerRating}
                onChange={(e) => update('heelerRating', e.target.value)}
                placeholder="5"
                required
                className="bg-muted border-border rounded-xl h-11"
              />
            </div>
          </div>

          {total > 0 && (
            <div className="p-4 bg-muted rounded-xl border border-border shadow-sm">
              <div className="flex justify-between items-center">
                <span className="text-muted-foreground">Rating total del equipo:</span>
                <span className={`text-2xl ${(maxRating && maxRating > 0 && total > maxRating) ? 'text-red-600' : 'text-foreground'}`}>
                  {total.toFixed(0)}
                </span>
              </div>
              {maxRating && maxRating > 0 && total > maxRating && (
                <p className="text-red-600 mt-2">⚠️ El rating excede el máximo permitido ({maxRating})</p>
              )}
            </div>
          )}

          <div className="flex justify-end gap-3 pt-4 border-t border-border">
            <Button type="button" variant="outline" onClick={onClose} disabled={submitting} className="border-border">
              Cancelar
            </Button>
            <Button type="submit" disabled={submitting || (!!maxRating && maxRating > 0 && total > maxRating)} className="bg-primary text-primary-foreground">
              {submitting ? 'Guardando…' : initialValue ? 'Actualizar' : 'Guardar'}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  )
}
