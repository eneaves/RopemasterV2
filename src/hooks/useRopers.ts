import { useCallback, useEffect, useState } from 'react'
import { toast } from 'sonner'
import { listRopers, createRoper, updateRoper, deleteRoper } from '@/lib/api'

type Roper = {
  id: number
  firstName: string
  lastName: string
  specialty: 'header' | 'heeler' | 'both'
  rating: number
  level: 'pro' | 'amateur' | 'principiante'
  phone?: string | null
  email?: string | null
  createdAt?: string
  updatedAt?: string
}

export function useRopers() {
  const [ropers, setRopers] = useState<Roper[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const mapRow = useCallback((r: any): Roper => ({
    id: Number(r.id),
    firstName: r.first_name ?? r.firstName ?? '',
    lastName: r.last_name ?? r.lastName ?? '',
    specialty: (r.specialty ?? 'both') as 'header' | 'heeler' | 'both',
    rating: typeof r.rating === 'number' ? r.rating : Number(r.rating ?? 0),
    level: (String(r.level ?? 'amateur').toLowerCase() as 'pro' | 'amateur' | 'principiante'),
    phone: r.phone ?? null,
    email: r.email ?? null,
    createdAt: r.created_at ?? r.createdAt,
    updatedAt: r.updated_at ?? r.updatedAt,
  }), [])

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const data = await listRopers()
      setRopers((data || []).map(mapRow))
    } catch (e: any) {
      const msg = String(e?.message ?? e)
      setError(msg)
      toast.error(msg)
    } finally {
      setLoading(false)
    }
  }, [mapRow])

  const add = useCallback(async (payload: {
    firstName: string
    lastName: string
    specialty: 'header' | 'heeler' | 'both'
    rating: number
    phone?: string | null
    email?: string | null
    level?: 'pro' | 'amateur' | 'principiante'
  }) => {
    setLoading(true)
    setError(null)
    try {
      const apiPayload = {
        first_name: payload.firstName,
        last_name: payload.lastName,
        specialty: payload.specialty,
        rating: payload.rating,
        phone: payload.phone ?? null,
        email: payload.email ?? null,
        level: (payload.level ?? 'amateur'),
      }
      await createRoper(apiPayload)
      toast.success('Roper creado')
      await refresh()
    } catch (e: any) {
      const msg = String(e?.message ?? e)
      setError(msg)
      toast.error(msg)
      throw e
    } finally {
      setLoading(false)
    }
  }, [refresh])

  const edit = useCallback(async (id: number, patch: Partial<{
    firstName: string
    lastName: string
    specialty: 'header' | 'heeler' | 'both'
    rating: number
    phone?: string | null
    email?: string | null
    level?: 'pro' | 'amateur' | 'principiante'
  }>) => {
    setLoading(true)
    setError(null)
    try {
      const apiPatch: any = {}
      if (patch.firstName !== undefined) apiPatch.first_name = patch.firstName
      if (patch.lastName !== undefined) apiPatch.last_name = patch.lastName
      if (patch.specialty !== undefined) apiPatch.specialty = patch.specialty
      if (patch.rating !== undefined) apiPatch.rating = patch.rating
      if (patch.phone !== undefined) apiPatch.phone = patch.phone
      if (patch.email !== undefined) apiPatch.email = patch.email
  if (patch.level !== undefined) apiPatch.level = patch.level

      await updateRoper(id, apiPatch)
      toast.success('Roper actualizado')
      await refresh()
    } catch (e: any) {
      const msg = String(e?.message ?? e)
      setError(msg)
      toast.error(msg)
      throw e
    } finally {
      setLoading(false)
    }
  }, [refresh])

  const remove = useCallback(async (id: number) => {
    setLoading(true)
    setError(null)
    try {
      await deleteRoper(id)
      toast.success('Roper eliminado')
      setRopers((prev) => prev.filter((r) => r.id !== id))
    } catch (e: any) {
      const msg = String(e?.message ?? e)
      setError(msg)
      toast.error(msg)
      throw e
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    refresh()
  }, [refresh])

  return { ropers, loading, error, refresh, add, edit, remove } as const
}
