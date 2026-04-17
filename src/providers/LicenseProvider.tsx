import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react'
import { getLicenseStatus } from '../lib/api'
import type { LicenseStatusDto } from '../types/license'

interface LicenseContextValue {
  status: LicenseStatusDto | null
  loading: boolean
  refreshing: boolean
  refresh: () => Promise<void>
  setStatus: (next: LicenseStatusDto | null) => void
  isActive: boolean
}

const LicenseContext = createContext<LicenseContextValue | null>(null)

export function LicenseProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<LicenseStatusDto | null>(null)
  const [loading, setLoading] = useState(true)
  const [refreshing, setRefreshing] = useState(false)

  const fetchStatus = useCallback(async () => {
    const response = await getLicenseStatus()
    setStatus(response)
  }, [])

  const refresh = useCallback(async () => {
    setRefreshing(true)
    try {
      await fetchStatus()
    } finally {
      setRefreshing(false)
    }
  }, [fetchStatus])

  useEffect(() => {
    fetchStatus()
      .catch((err) => {
        // eslint-disable-next-line no-console
        console.error('[LicenseProvider] No se pudo obtener el estado de la licencia', err)
      })
      .finally(() => setLoading(false))
  }, [fetchStatus])

  const value = useMemo<LicenseContextValue>(() => {
    const isActive = status?.status === 'active' && !status?.is_placeholder
    return {
      status,
      loading,
      refreshing,
      refresh,
      setStatus,
      isActive,
    }
  }, [status, loading, refreshing, refresh])

  return <LicenseContext.Provider value={value}>{children}</LicenseContext.Provider>
}

export function useLicense() {
  const ctx = useContext(LicenseContext)
  if (!ctx) {
    throw new Error('useLicense must be used within a LicenseProvider')
  }
  return ctx
}
