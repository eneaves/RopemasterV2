import { RefreshCcw, ShieldAlert } from 'lucide-react'
import { Button } from './ui/button'
import { LicensePanel } from './LicensePanel'
import { useLicense } from '../providers/LicenseProvider'

export function LicenseGate() {
  const { status, loading, refresh } = useLicense()

  if (loading) {
    return (
      <div className="min-h-screen w-full flex flex-col items-center justify-center bg-background text-foreground gap-4">
        <ShieldAlert className="size-10 text-orange-500 animate-pulse" />
        <p className="text-sm text-muted-foreground">Verificando licencia…</p>
      </div>
    )
  }

  const message = gateMessage(status?.status)

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
              onClick={() => refresh().catch(() => {})}
              className="inline-flex items-center gap-2"
            >
              <RefreshCcw className="size-4" />
              Reintentar verificación
            </Button>
          </div>
        </div>
        <div className="bg-card border border-border rounded-2xl shadow-lg p-6">
          <LicensePanel />
        </div>
      </div>
    </div>
  )
}

function gateMessage(status?: string) {
  switch (status) {
    case 'expired':
      return 'La licencia instalada ha expirado. Instala una nueva para desbloquear todas las funciones.'
    case 'not_yet_valid':
      return 'La licencia aún no es válida en este dispositivo. Verifica la hora del sistema o contacta a soporte.'
    case 'invalid_device':
      return 'La licencia instalada pertenece a otro dispositivo. Solicita una licencia para este equipo.'
    case 'active':
      return 'Licencia verificada.'
    default:
      return 'No se detectó una licencia válida. Todas las funciones permanecerán bloqueadas hasta instalar una.'
  }
}
