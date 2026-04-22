import { useCallback, useEffect, useMemo, useState } from 'react'
import { toast } from 'sonner'
import {
  EventRosterSyncEntry,
  SyncEventRosterResult,
  listEventRoster,
  syncEventRoster,
  updateEventRosterEntry,
} from '@/lib/api'

export type EventRosterEntry = {
  id: number
  eventId: number
  roperId: number
  status: 'registered' | 'confirmed' | 'withdrawn'
  ratingOverride?: number | null
  sourceHash?: string | null
  notes?: string | null
  firstName: string
  lastName: string
  specialty: 'header' | 'heeler' | 'both'
  rating: number
  level: 'pro' | 'amateur' | 'principiante'
  phone?: string | null
  email?: string | null
}

const mapRow = (row: any): EventRosterEntry => ({
  id: Number(row.id),
  eventId: Number(row.event_id),
  roperId: Number(row.roper_id),
  status: (row.status ?? 'registered') as EventRosterEntry['status'],
  ratingOverride: row.rating_override ?? null,
  sourceHash: row.source_hash ?? null,
  notes: row.notes ?? null,
  firstName: row.first_name ?? '',
  lastName: row.last_name ?? '',
  specialty: (row.specialty ?? 'both') as 'header' | 'heeler' | 'both',
  rating: Number(row.rating ?? 0),
  level: (String(row.level ?? 'amateur').toLowerCase() as 'pro' | 'amateur' | 'principiante'),
  phone: row.phone ?? null,
  email: row.email ?? null,
})

export function useEventRoster(eventId?: number) {
  const [roster, setRoster] = useState<EventRosterEntry[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(
    async (includeWithdrawn = true) => {
      if (!eventId || Number.isNaN(eventId)) {
        setRoster([])
        return []
      }
      setLoading(true)
      setError(null)
      try {
        const data = await listEventRoster(eventId, { includeWithdrawn })
        const mapped = (data || []).map(mapRow)
        setRoster(mapped)
        return mapped
      } catch (e: any) {
        const msg = String(e?.message ?? e)
        setError(msg)
        toast.error(msg)
        throw e
      } finally {
        setLoading(false)
      }
    },
    [eventId],
  )

  useEffect(() => {
    if (!eventId || Number.isNaN(eventId)) {
      setRoster([])
      return
    }
    refresh(true)
  }, [eventId, refresh])

  const sync = useCallback(
    async (entries: EventRosterSyncEntry[], withdrawAbsent = true) => {
      if (!eventId || Number.isNaN(eventId)) {
        throw new Error('No hay eventId válido para sincronizar el roster.')
      }
      if (!entries || entries.length === 0) {
        return { created_ropers: 0, updated_ropers: 0, reactivated_ropers: 0, roster_upserts: 0, roster_marked_withdrawn: 0 } as SyncEventRosterResult
      }
      setLoading(true)
      setError(null)
      try {
        const result = await syncEventRoster({
          event_id: eventId,
          entries,
          withdraw_absent: withdrawAbsent,
        })
        toast.success(`Roster sincronizado (${result.roster_upserts} registros)`)
        await refresh(true)
        return result
      } catch (e: any) {
        const msg = String(e?.message ?? e)
        setError(msg)
        toast.error(msg)
        throw e
      } finally {
        setLoading(false)
      }
    },
    [eventId, refresh],
  )

  const updateEntry = useCallback(
    async (payload: {
      id: number
      status?: 'registered' | 'confirmed' | 'withdrawn'
      rating_override?: number | null
      notes?: string | null
    }) => {
      if (!payload?.id) return
      try {
        await updateEventRosterEntry({
          id: payload.id,
          status: payload.status,
          rating_override: payload.rating_override ?? undefined,
          notes: payload.notes,
        })
        await refresh(true)
        toast.success('Roster actualizado')
      } catch (e: any) {
        const msg = String(e?.message ?? e)
        setError(msg)
        toast.error(msg)
        throw e
      }
    },
    [refresh],
  )

  const activeRoster = useMemo(
    () => roster.filter((r) => r.status !== 'withdrawn'),
    [roster],
  )

  const confirmedRoster = useMemo(
    () => roster.filter((r) => r.status === 'confirmed'),
    [roster],
  )

  return {
    roster,
    activeRoster,
    confirmedRoster,
    loading,
    error,
    refresh,
    sync,
    updateEntry,
  } as const
}
