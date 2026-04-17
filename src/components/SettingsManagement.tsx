import { useMemo, useState } from 'react'
import { Button } from './ui/button'
import { Input } from './ui/input'
import { Switch } from './ui/switch'
import { toast } from 'sonner'
import { LicensePanel } from './LicensePanel'
import { useLicense } from '../providers/LicenseProvider'
import { Loader2 } from 'lucide-react'
import { getLicenseBadge, getLicenseSummaryMessage, maskDeviceHash } from '../lib/license-ui'

const tabs = [
  'Perfil de usuario',
  'Datos y almacenamiento',
  'Sistema / Avanzado',
  'Licencia',
]

export function SettingsManagement() {
  const [active, setActive] = useState(tabs[0])
  const [clearTempOnClose, setClearTempOnClose] = useState(false)
  const { status: licenseSummary, loading: loadingLicense, refreshing, refresh } = useLicense()
  const hasLicense = Boolean(licenseSummary && !licenseSummary.is_placeholder)

  const badge = useMemo(() => {
    return getLicenseBadge(licenseSummary?.status)
  }, [licenseSummary])

  const formatDate = (value?: number | null) => {
    if (!value) return '—'
    return new Date(value * 1000).toLocaleDateString('es-MX', {
      year: 'numeric',
      month: 'short',
      day: '2-digit',
    })
  }

  return (
    <div className="p-6 h-full">
      <div className="max-w-full">
        <div className="mb-6 flex items-start justify-between">
          <div>
            <h1 className="text-2xl font-semibold text-foreground">Configuración</h1>
            <p className="text-sm text-muted-foreground">Administra tus preferencias, datos y opciones generales de la aplicación</p>
          </div>
        </div>

        <div className="bg-card border border-border rounded-xl p-4 flex gap-6">
          <aside className="w-56 border-r border-border pr-4">
            <nav className="flex flex-col gap-2">
              {tabs.map((t) => (
                <button key={t} onClick={() => setActive(t)} className={`w-full text-left px-3 py-2 rounded-md ${active === t ? 'bg-orange-50 text-orange-700' : 'hover:bg-muted/50'}`}>
                  {t}
                </button>
              ))}
            </nav>
          </aside>

          <section className="flex-1">
            {active === 'Perfil de usuario' && (
              <div>
                <h2 className="text-lg font-medium">Resumen de Licencia</h2>
                <p className="text-sm text-muted-foreground">Información rápida de tu activación actual.</p>

                <div className="mt-6 border border-border rounded-xl p-5 bg-card shadow-sm">
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <div>
                      <p className="text-xs uppercase tracking-wide text-muted-foreground">Estado</p>
                      <p className="text-2xl font-semibold text-foreground">
                        {licenseSummary ? badge.label : loadingLicense ? 'Cargando...' : 'Sin licencia'}
                      </p>
                    </div>
                    <span className={`px-3 py-1 rounded-full text-sm font-medium ${badge.className}`}>
                      {badge.label}
                    </span>
                  </div>

                  <p className="mt-3 text-sm text-muted-foreground">
                    {licenseSummary ? getLicenseSummaryMessage(licenseSummary) : 'Instala una licencia válida para activar Roping Manager.'}
                  </p>

                  {hasLicense ? (
                    <div className="mt-6 grid gap-4 md:grid-cols-2">
                      <InfoTile label="Titular" value={licenseSummary?.customer_name ?? '—'} />
                      <InfoTile label="Plan" value={licenseSummary?.plan ?? '—'} />
                      <InfoTile label="Vigencia" value={formatDate(licenseSummary?.not_before)} />
                      <InfoTile label="Expira" value={formatDate(licenseSummary?.not_after)} />
                      <InfoTile label="License ID" value={licenseSummary?.license_id ?? '—'} mono />
                      <InfoTile label="Device hash" value={maskDeviceHash(licenseSummary?.device_hash_hex)} mono />
                    </div>
                  ) : (
                    <p className="mt-4 text-sm text-muted-foreground">
                      Instala una licencia válida para ver los detalles de activación.
                    </p>
                  )}

                  <div className="mt-6 flex flex-wrap gap-3">
                    <Button
                      onClick={() =>
                        refresh().catch((err) =>
                          toast.error(err instanceof Error ? err.message : 'No se pudo actualizar'),
                        )
                      }
                      variant="outline"
                      disabled={loadingLicense || refreshing}
                      className="inline-flex items-center gap-2"
                    >
                      {loadingLicense || refreshing ? <Loader2 className="size-4 animate-spin" /> : null}
                      {loadingLicense || refreshing ? 'Actualizando...' : 'Actualizar'}
                    </Button>
                    <Button variant="secondary" onClick={() => setActive('Licencia')}>
                      Abrir panel de licencias
                    </Button>
                  </div>
                </div>
              </div>
            )}

            {active === 'Datos y almacenamiento' && (
              <div>
                <h2 className="text-lg font-medium">Gestión de Datos y Base de Datos</h2>
                <p className="text-sm text-muted-foreground">Administra tus datos y realiza respaldos</p>

                <div className="mt-6 bg-muted/50 p-4 rounded-md">
                  <div className="text-sm text-muted-foreground">Ubicación de la base de datos</div>
                  <div className="mt-2 flex gap-3 items-center">
                    <Input value={'/Users/Emiliano/roping-manager/data/main.db'} onChange={() => {}} />
                    <Button variant="outline">Cambiar ubicación</Button>
                  </div>
                </div>

                <div className="mt-4 flex gap-3">
                  <Button className="bg-orange-500 hover:bg-orange-600 text-white">Exportar respaldo</Button>
                  <Button variant="outline">Importar respaldo</Button>
                </div>

                <div className="mt-4 border-t pt-4">
                  <div className="flex items-center justify-between">
                    <div>
                      <div className="text-sm">Borrar datos temporales al cerrar la app</div>
                      <div className="text-xs text-muted-foreground">Limpia archivos temporales automáticamente</div>
                    </div>
                    <Switch checked={clearTempOnClose} onCheckedChange={(v) => setClearTempOnClose(Boolean(v))} />
                  </div>

                  <div className="mt-3 text-sm text-muted-foreground">Tamaño actual de la base de datos <span className="ml-2 font-medium">24 MB</span></div>
                </div>
              </div>
            )}

            {active === 'Licencia' && (
              <div>
                <h2 className="text-lg font-medium">Licencia</h2>
                <p className="text-sm text-muted-foreground">Gestiona la licencia de Roping Manager.</p>
                <div className="mt-6">
                  <LicensePanel />
                </div>
              </div>
            )}

            {active === 'Sistema / Avanzado' && (
              <div>
                <h2 className="text-lg font-medium">Sistema / Avanzado</h2>
                <p className="text-sm text-muted-foreground">Información del sistema y opciones avanzadas</p>

                <div className="mt-6 bg-muted/50 p-4 rounded-md">
                  <div className="flex items-center justify-between">
                    <div>
                      <div className="text-sm text-muted-foreground">Versión de la aplicación</div>
                      <div className="font-medium">v1.0.0</div>
                    </div>
                    <Button variant="outline">Buscar actualizaciones</Button>
                  </div>
                </div>

                <div className="mt-4 text-sm text-muted-foreground">Roping Manager usa SQLite y Tauri para ofrecer un entorno rápido y local sin conexión.</div>
              </div>
            )}
          </section>
        </div>
      </div>
    </div>
  )
}

function InfoTile({
  label,
  value,
  mono,
}: {
  label: string
  value: string
  mono?: boolean
}) {
  return (
    <div className="rounded-lg border border-border p-3">
      <div className="text-xs uppercase tracking-wide text-muted-foreground">{label}</div>
      <div className={`text-sm mt-1 ${mono ? 'font-mono break-all' : 'font-medium text-foreground'}`}>
        {value || '—'}
      </div>
    </div>
  )
}
