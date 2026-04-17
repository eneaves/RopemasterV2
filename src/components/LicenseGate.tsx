import { Loader2, RefreshCcw, ShieldAlert } from 'lucide-react'
import { Button } from './ui/button'
import { LicensePanel } from './LicensePanel'
import { useLicense } from '../providers/LicenseProvider'
import { getLicenseGateMessage } from '../lib/license-ui'

export function LicenseGate() {
  const { status, loading, refresh, refreshing } = useLicense()

  if (loading) {
    return (
      <div className="min-h-screen w-full flex flex-col items-center justify-center bg-background text-foreground gap-4">
        <ShieldAlert className="size-10 text-orange-500 animate-pulse" />
        <p className="text-sm text-muted-foreground">Verificando licencia…</p>
      </div>
    )
  }

  const message = getLicenseGateMessage(status?.status)

  return (
    <div className="min-h-screen w-full bg-muted/30 flex flex-col items-center justify-center px-4 py-10">
      <div className="max-w-5xl w-full space-y-8">
        <div className="text-center space-y-3">
          <h1 className="text-2xl font-semibold text-foreground">Licencia requerida</h1>
          <p className="text-sm text-muted-foreground">{message}</p>
          <div className="flex justify-center">
            <Button
              variant="outline"
              size="sm"
              disabled={refreshing}
              onClick={() => refresh().catch(() => {})}
              className="inline-flex items-center gap-2"
            >
              {refreshing ? <Loader2 className="size-4 animate-spin" /> : <RefreshCcw className="size-4" />}
              {refreshing ? 'Actualizando...' : 'Reintentar verificación'}
            </Button>
          </div>
        </div>
        <div className="bg-card border border-border rounded-2xl shadow-lg p-6">
          <LicensePanel variant="gate" />
        </div>
      </div>
    </div>
  )
}
