import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react'
import { getEvents, getSeries } from '@/lib/api'

type SeriesRecord = Record<string, any>
type EventRecord = Record<string, any>

interface SeriesEventsContextValue {
  series: SeriesRecord[]
  events: EventRecord[]
  loading: boolean
  loadingSeries: boolean
  loadingEvents: boolean
  error: string | null
  refreshSeries: () => Promise<void>
  refreshEvents: () => Promise<void>
  refreshAll: () => Promise<void>
  getEventsForSeries: (seriesId?: string | number | null) => EventRecord[]
}

const SeriesEventsContext = createContext<SeriesEventsContextValue | null>(null)

export function SeriesEventsProvider({ children }: { children: ReactNode }) {
  const [series, setSeries] = useState<SeriesRecord[]>([])
  const [events, setEvents] = useState<EventRecord[]>([])
  const [loadingSeries, setLoadingSeries] = useState(true)
  const [loadingEvents, setLoadingEvents] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refreshSeries = useCallback(async () => {
    setLoadingSeries(true)
    try {
      const data = await getSeries()
      setSeries(Array.isArray(data) ? data : [])
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoadingSeries(false)
    }
  }, [])

  const refreshEvents = useCallback(async () => {
    setLoadingEvents(true)
    try {
      const data = await getEvents()
      setEvents(Array.isArray(data) ? data : [])
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoadingEvents(false)
    }
  }, [])

  const refreshAll = useCallback(async () => {
    await Promise.all([refreshSeries(), refreshEvents()])
  }, [refreshSeries, refreshEvents])

  useEffect(() => {
    refreshAll().catch(() => {
      /* errors already stored in state */
    })
  }, [refreshAll])

  const getEventsForSeries = useCallback(
    (seriesId?: string | number | null) => {
      if (seriesId === undefined || seriesId === null || seriesId === '' || seriesId === 'all') {
        return events
      }
      const idStr = String(seriesId)
      return events.filter((event) => String(event.series_id ?? event.seriesId) === idStr)
    },
    [events],
  )

  const value = useMemo<SeriesEventsContextValue>(
    () => ({
      series,
      events,
      loading: loadingSeries || loadingEvents,
      loadingSeries,
      loadingEvents,
      error,
      refreshSeries,
      refreshEvents,
      refreshAll,
      getEventsForSeries,
    }),
    [
      series,
      events,
      loadingSeries,
      loadingEvents,
      error,
      refreshSeries,
      refreshEvents,
      refreshAll,
      getEventsForSeries,
    ],
  )

  return <SeriesEventsContext.Provider value={value}>{children}</SeriesEventsContext.Provider>
}

export function useSeriesEvents() {
  const ctx = useContext(SeriesEventsContext)
  if (!ctx) {
    throw new Error('useSeriesEvents must be used within a SeriesEventsProvider')
  }
  return ctx
}
