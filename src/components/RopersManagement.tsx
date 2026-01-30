import { useState, useRef } from 'react'
import { Button } from './ui/button'
import { Input } from './ui/input'
import { Label } from './ui/label'
import {
  Table,
  TableHeader,
  TableBody,
  TableRow,
  TableHead,
  TableCell,
} from './ui/table'
import { Badge } from './ui/badge'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogFooter } from './ui/dialog'
import { useRopers } from '@/hooks/useRopers'
import * as XLSX from 'xlsx'
import { createRoper } from '@/lib/api'
import { toast } from 'sonner'

// now sourced from DB via hook

export function RopersManagement() {
  const [query, setQuery] = useState('')
  const { ropers, add, edit, remove, removeAll, refresh } = useRopers()
  const fileRef = useRef<HTMLInputElement | null>(null)
  const [importing, setImporting] = useState(false)
  const [categoryFilter, setCategoryFilter] = useState<'all'|'pro'|'amateur'|'principiante'>('all')
  const [roleFilter, setRoleFilter] = useState<'all'|'header'|'heeler'|'both'>('all')
  const [statusFilter, setStatusFilter] = useState<'all'|'active'|'inactive'>('all')

      // Modal / form state
      const [isModalOpen, setIsModalOpen] = useState(false)
      const [editing, setEditing] = useState<any | null>(null)
  const [isDeleteModalOpen, setIsDeleteModalOpen] = useState(false)
  const [deleteCandidate, setDeleteCandidate] = useState<any | null>(null)
  const [isDeleteAllModalOpen, setIsDeleteAllModalOpen] = useState(false)
      const [firstName, setFirstName] = useState('')
      const [lastName, setLastName] = useState('')
      const [specialty, setSpecialty] = useState<'header'|'heeler'|'both'>('both')
      const [rating, setRating] = useState<number>(0)
      const [phone, setPhone] = useState<string | null>(null)
      const [email, setEmail] = useState<string | null>(null)
      const [level, setLevel] = useState<'pro'|'amateur'|'principiante'>('amateur')

      const openCreate = () => {
        setEditing(null)
        setFirstName('')
        setLastName('')
        setSpecialty('both')
        setRating(0)
        setPhone(null)
        setEmail(null)
        setLevel('amateur')
        setIsModalOpen(true)
      }

      const openEdit = (r: any) => {
        setEditing(r)
        setFirstName(r.firstName ?? '')
        setLastName(r.lastName ?? '')
        setSpecialty(r.specialty ?? 'both')
        setRating(Number(r.rating ?? 0))
        setPhone(r.phone ?? null)
        setEmail(r.email ?? null)
        setLevel((r.level ?? 'amateur') as 'pro'|'amateur'|'principiante')
        setIsModalOpen(true)
      }

      const onSubmit = async () => {
        // validaciones mínimas
        if (!firstName.trim()) {
          toast.error('Nombre es obligatorio')
          return
        }

        try {
          if (editing && editing.id) {
            await edit(editing.id, { firstName, lastName, specialty, rating, phone, email, level })
          } else {
            await add({ firstName, lastName, specialty, rating, phone, email, level })
          }
          setIsModalOpen(false)
        } catch (e) {
          // errors ya manejados por hook (toasts), pero cerramos modal solo si éxito
        }
      }

      const handleDeleteRequest = (r: any) => {
        // Open confirmation modal
        setDeleteCandidate(r)
        setIsDeleteModalOpen(true)
      }

      

      async function parseRopersFile(file: File) {
        const buf = await file.arrayBuffer()
        const wb = XLSX.read(buf, { type: 'array' })
        const ws = wb.Sheets[wb.SheetNames[0]]
        const json: any[] = XLSX.utils.sheet_to_json<any>(ws, { defval: '' })

        const norm = (s: string) => String(s || '').trim().toLowerCase()
        const map = (r: any) => {
          const get = (keys: string[]) => {
            for (const k of keys) { if (r[k] != null && r[k] !== '') return String(r[k]) }
            return ''
          }
          const first = get(['first_name','nombre','name','first'])
          const last  = get(['last_name','apellido','apellidos','last'])
          const specialtyRaw = norm(get(['specialty','especialidad','role','posicion']))
          const levelRaw = norm(get(['level','nivel','categoria']))
          const ratingRaw = get(['rating','handicap','puntaje'])

          const specialty: any =
            specialtyRaw.includes('header') || specialtyRaw.includes('cabeza') ? 'header' :
            specialtyRaw.includes('heeler') || specialtyRaw.includes('pial')   ? 'heeler' :
            specialtyRaw.includes('both')   || specialtyRaw.includes('ambos')  ? 'both'   : undefined

          const level: any =
            levelRaw === 'pro' ? 'pro' :
            levelRaw === 'principiante' || levelRaw === 'beginner' ? 'principiante' :
            levelRaw === 'amateur' ? 'amateur' : undefined

          return {
            first_name: first,
            last_name:  last,
            specialty,
            rating: ratingRaw ? Number(ratingRaw) : undefined,
            phone: get(['phone','telefono','tel']),
            email: get(['email','correo']),
            level,
          }
        }

        const rows = json.map(map)
        const cleaned = rows.filter(r => r.first_name)
        if (cleaned.length === 0) throw new Error('El archivo no contiene ropers válidos (se requiere nombre).')
        return cleaned
      }

      async function importRopers(rows: any[]) {
        const existing = ropers || []
        const norm = (s: any) => String(s ?? '').trim().toLowerCase()
        const seenEmail = new Set(existing.map((r: any) => norm(r.email)))
        const seenName  = new Set(existing.map((r: any) => `${norm(r.firstName)}|${norm(r.lastName)}`))

        let created = 0, duplicates = 0, errors = 0

        for (const r of rows) {
          const emailKey = norm(r.email)
          const nameKey  = `${norm(r.first_name)}|${norm(r.last_name)}`
          if ((emailKey && seenEmail.has(emailKey)) || seenName.has(nameKey)) { duplicates++; continue }

          const payload = {
            first_name: r.first_name,
            last_name: r.last_name,
            specialty: r.specialty ?? 'both',
            rating: Number.isFinite(r.rating) ? r.rating : 0,
            phone: r.phone ?? null,
            email: r.email ?? null,
            level: r.level ?? 'amateur',
          }

          try {
            await createRoper(payload)
            created++
            if (emailKey) seenEmail.add(emailKey)
            seenName.add(nameKey)
          } catch (e) {
            errors++
          }
        }
        // refresh once
        try { await refresh() } catch {}
        return { created, duplicates, errors }
      }

      const handleFile = async (e: React.ChangeEvent<HTMLInputElement>) => {
        const file = e.target.files?.[0]
        if (!file) return
        setImporting(true)
        try {
          const rows = await parseRopersFile(file)
          const report = await importRopers(rows)
          toast.success(`Importados: ${report.created} | Duplicados: ${report.duplicates} | Errores: ${report.errors}`)
        } catch (err: any) {
          toast.error(err?.message ?? 'Error al importar')
        } finally {
          e.target.value = ''
          setImporting(false)
        }
      }

      const handleConfirmDelete = async () => {
        if (!deleteCandidate) return
        try {
          await remove(deleteCandidate.id)
          toast.success('Roper eliminado')
          setIsDeleteModalOpen(false)
          setDeleteCandidate(null)
        } catch (e: any) {
          const msg = String(e?.message ?? e)
          if (msg.toLowerCase().includes('usado en equipos') || msg.toLowerCase().includes('está usado')) {
            toast.error('No se puede eliminar: el roper está asignado a equipos')
          } else {
            toast.error(msg)
          }
        }
      }

      const handleCancelDelete = () => {
        setIsDeleteModalOpen(false)
        setDeleteCandidate(null)
      }

      const handleDeleteAllRequest = () => {
        setIsDeleteAllModalOpen(true)
      }

      const handleConfirmDeleteAll = async () => {
        try {
          await removeAll()
          setIsDeleteAllModalOpen(false)
        } catch (e: any) {
          // Error ya manejado por el hook con toast
        }
      }

      const handleCancelDeleteAll = () => {
        setIsDeleteAllModalOpen(false)
      }

      const q = query.trim().toLowerCase()
      const filtered = ropers
        .filter((r) => {
          if (!q) return true
          const name = `${r.firstName} ${r.lastName}`.toLowerCase()
          const specialty = (r.specialty ?? '').toLowerCase()
          const levelStr = String(r.level ?? '').toLowerCase()
          const email = String(r.email ?? '').toLowerCase()
          const ratingStr = String(r.rating ?? '')

          return (
            name.includes(q) ||
            specialty.includes(q) ||
            levelStr.includes(q) ||
            email.includes(q) ||
            ratingStr === q
          )
        })
        .filter((r) => (categoryFilter === 'all' ? true : String(r.level ?? 'amateur') === categoryFilter))
        .filter((r) => {
          if (roleFilter === 'all') return true
          const s = (r.specialty ?? 'both').toLowerCase()
          if (roleFilter === 'header') return s === 'header' || s === 'both'
          if (roleFilter === 'heeler') return s === 'heeler' || s === 'both'
          if (roleFilter === 'both') return s === 'both'
          return true
        })
        .filter((r) => (statusFilter === 'all' ? true : ((r as any).status ? String((r as any).status) === statusFilter : true)))

      return (
        <div className="p-6 h-full">
          <div className="mb-6 flex items-start justify-between">
            <div>
              <h1 className="text-2xl font-semibold text-foreground">Gestión de Ropers</h1>
              <p className="text-sm text-muted-foreground">Administra competidores y sus capacidades</p>
            </div>

            <div className="flex items-center gap-3">
                <Button variant="outline" className="rounded-md" onClick={() => fileRef.current?.click()} disabled={importing}>Importar Excel</Button>
                <input ref={fileRef} type="file" accept=".xlsx,.xls,.csv" hidden onChange={handleFile} />
              <Button variant="destructive" className="rounded-md" onClick={handleDeleteAllRequest}>Borrar Todos</Button>
              <Button onClick={openCreate} className="bg-primary text-primary-foreground rounded-md">+ Agregar Nuevo Roper</Button>
            </div>
          </div>

          <div className="bg-card border border-border rounded-xl p-4">
            <h3 className="text-lg font-medium mb-2">Directorio de Ropers</h3>
            <p className="text-sm text-muted-foreground mb-4">Busca y filtra competidores ({ropers.length} ropers)</p>

              <div className="mb-4 flex items-center gap-3">
              <div className="flex-1">
                <Input placeholder="Buscar por nombre..." value={query} onChange={(e) => setQuery(e.target.value)} />
              </div>

              <select value={categoryFilter} onChange={(e) => setCategoryFilter(e.target.value as any)} className="rounded-lg border border-border bg-card px-3 py-2 text-sm">
                <option value="all">Todas las Categorías</option>
                <option value="pro">Pro</option>
                <option value="amateur">Amateur</option>
                <option value="principiante">Principiante</option>
              </select>

              <select value={roleFilter} onChange={(e) => setRoleFilter(e.target.value as any)} className="rounded-lg border border-border bg-card px-3 py-2 text-sm">
                <option value="all">Todos los Roles</option>
                <option value="header">Headers</option>
                <option value="heeler">Heelers</option>
                <option value="both">Ambos (Both)</option>
              </select>

              <select value={statusFilter} onChange={(e) => setStatusFilter(e.target.value as any)} className="rounded-lg border border-border bg-card px-3 py-2 text-sm">
                <option value="all">Todos los Estados</option>
                <option value="active">Activo</option>
                <option value="inactive">Inactivo</option>
              </select>
            </div>

            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Nombre</TableHead>
                  <TableHead>Categoría</TableHead>
                  <TableHead>Nivel</TableHead>
                  <TableHead>Rating</TableHead>
                  <TableHead>¿Puede Header?</TableHead>
                  <TableHead>¿Puede Heeler?</TableHead>
                  <TableHead>Estatus</TableHead>
                  <TableHead>Última Participación</TableHead>
                  <TableHead>Acciones</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {filtered.map((r) => (
                  <TableRow key={r.id}>
                    <TableCell className="font-medium">{`${r.firstName} ${r.lastName}`}</TableCell>
                    <TableCell><span className="inline-flex items-center rounded-full bg-muted/20 px-2 py-1 text-xs">{(r.specialty ?? '').toUpperCase()}</span></TableCell>
                    <TableCell>
                      <span className="inline-flex items-center rounded-full bg-muted/20 px-2 py-1 text-xs">{r.level ?? 'amateur'}</span>
                    </TableCell>
                    <TableCell>
                      <Badge className="bg-muted text-muted-foreground">{typeof r.rating === 'number' ? String(r.rating) : (r.rating ?? '-')}</Badge>
                    </TableCell>
                    <TableCell>{r.specialty !== 'heeler' ? <Badge className="bg-green-50 text-green-700">Sí</Badge> : <Badge className="bg-muted text-muted-foreground">No</Badge>}</TableCell>
                    <TableCell>{r.specialty !== 'header' ? <Badge className="bg-green-50 text-green-700">Sí</Badge> : <Badge className="bg-muted text-muted-foreground">No</Badge>}</TableCell>
                    <TableCell><Badge className="bg-green-50 text-green-700">Activo</Badge></TableCell>
                    <TableCell className="text-sm text-muted-foreground">{r.updatedAt ?? '-'}</TableCell>
                    <TableCell className="text-right">
                      <div className="inline-flex items-center gap-2">
                        <Button variant="outline" size="sm" onClick={() => openEdit(r)}>Editar</Button>
                        <Button variant="destructive" size="sm" onClick={() => handleDeleteRequest(r)}>Borrar</Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>


          {/* Modal Crear/Editar Roper */}
          <Dialog open={isModalOpen} onOpenChange={setIsModalOpen}>
            <DialogContent className="sm:max-w-[500px]">
              <DialogHeader>
                <DialogTitle>{editing ? 'Editar Roper' : 'Crear Nuevo Roper'}</DialogTitle>
                <DialogDescription>{editing ? 'Actualiza la información del roper' : 'Completa los datos para crear un nuevo roper'}</DialogDescription>
              </DialogHeader>

              <div className="space-y-3 py-2">
                <div className="grid grid-cols-2 gap-3">
                  <Input placeholder="Nombre" value={firstName} onChange={(e) => setFirstName(e.target.value)} />
                  <Input placeholder="Apellido" value={lastName} onChange={(e) => setLastName(e.target.value)} />
                </div>

                <div className="grid grid-cols-2 gap-3">
                  <select value={specialty} onChange={(e) => setSpecialty(e.target.value as any)} className="rounded-lg border border-border bg-card px-3 py-2 text-sm">
                    <option value="header">Header</option>
                    <option value="heeler">Heeler</option>
                    <option value="both">Both</option>
                  </select>

                  <div className="space-y-2">
                    <Label htmlFor="rating">Rating</Label>
                    <Input id="rating" type="number" placeholder="Rating" value={String(rating)} onChange={(e) => setRating(Number(e.target.value))} />
                  </div>
                </div>

                <div className="grid grid-cols-2 gap-3">
                  <Input placeholder="Teléfono" value={phone ?? ''} onChange={(e) => setPhone(e.target.value || null)} />
                  <Input placeholder="Email" value={email ?? ''} onChange={(e) => setEmail(e.target.value || null)} />
                </div>
                <div className="grid grid-cols-1 gap-3">
                  <label className="text-sm">Nivel</label>
                  <select value={level} onChange={(e) => setLevel(e.target.value as any)} className="rounded-lg border border-border bg-card px-3 py-2 text-sm">
                    <option value="pro">Pro</option>
                    <option value="amateur">Amateur</option>
                    <option value="principiante">Principiante</option>
                  </select>
                </div>
              </div>

              <DialogFooter>
                <Button variant="outline" onClick={() => setIsModalOpen(false)} className="border-border">Cancelar</Button>
                <Button onClick={onSubmit} className="bg-primary text-primary-foreground">{editing ? 'Guardar' : 'Crear'}</Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>

          {/* Confirmación de borrado */}
          <Dialog open={isDeleteModalOpen} onOpenChange={setIsDeleteModalOpen}>
            <DialogContent className="sm:max-w-[420px]">
              <DialogHeader>
                <DialogTitle>Confirmar eliminación</DialogTitle>
                <DialogDescription>
                  ¿Estás seguro de que quieres eliminar a {deleteCandidate ? `${deleteCandidate.firstName} ${deleteCandidate.lastName}` : 'este roper'}? Esta acción no se puede deshacer.
                </DialogDescription>
              </DialogHeader>

              <div className="pt-4" />
              <DialogFooter>
                <Button variant="outline" onClick={handleCancelDelete} className="border-border">Cancelar</Button>
                <Button onClick={handleConfirmDelete} className="bg-destructive text-primary-foreground">Eliminar</Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>

          {/* Confirmación de borrado de todos los ropers */}
          <Dialog open={isDeleteAllModalOpen} onOpenChange={setIsDeleteAllModalOpen}>
            <DialogContent className="sm:max-w-[500px]">
              <DialogHeader>
                <DialogTitle>Confirmar eliminación masiva</DialogTitle>
                <DialogDescription>
                  ¿Estás seguro de que quieres eliminar TODOS los ropers ({ropers.length} en total)? Esta acción no se puede deshacer.
                </DialogDescription>
              </DialogHeader>

              <div className="bg-destructive/10 border border-destructive/20 rounded-lg p-4 my-4">
                <p className="text-sm font-medium text-destructive">⚠️ Advertencia</p>
                <p className="text-sm text-muted-foreground mt-1">
                  Esta acción eliminará permanentemente todos los ropers del sistema. Asegúrate de tener un respaldo antes de continuar.
                </p>
              </div>

              <DialogFooter>
                <Button variant="outline" onClick={handleCancelDeleteAll} className="border-border">Cancelar</Button>
                <Button onClick={handleConfirmDeleteAll} className="bg-destructive text-primary-foreground">Sí, eliminar todos</Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>
        </div>
      )
    }
