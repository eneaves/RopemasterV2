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
  refresh: () => Promise<void>
  setStatus: (next: LicenseStatusDto | null) => void
  isActive: boolean
}

const LicenseContext = createContext<LicenseContextValue | null>(null)

export function LicenseProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<LicenseStatusDto | null>(null)
  const [loading, setLoading] = useState(true)

  const refresh = useCallback(async () => {
    setLoading(true)
    try {
      const response = await getLicenseStatus()
      setStatus(response)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    refresh().catch((err) => {
      // eslint-disable-next-line no-console
      console.error('[LicenseProvider] No se pudo obtener el estado de la licencia', err)
    })
  }, [refresh])

  const value = useMemo<LicenseContextValue>(() => {
    const isActive = status?.status === 'active'
    return {
      status,
      loading,
      refresh,
      setStatus,
      isActive,
    }
  }, [status, loading, refresh])

  return <LicenseContext.Provider value={value}>{children}</LicenseContext.Provider>
}

export function useLicense() {
  const ctx = useContext(LicenseContext)
  if (!ctx) {
    throw new Error('useLicense must be used within a LicenseProvider')
  }
  return ctx
}
