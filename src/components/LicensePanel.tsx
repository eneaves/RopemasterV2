import { useEffect, useMemo, useState } from 'react'
import { open, save } from '@tauri-apps/plugin-dialog'
import { revealItemInDir } from '@tauri-apps/plugin-opener'
import { toast } from 'sonner'
import { Copy, FileDown, FolderOpen, Loader2, RefreshCcw, Trash2, Upload } from 'lucide-react'

import { Button } from './ui/button'
import { Input } from './ui/input'

import {
  getDeviceHash,
  generateLicenseRequest,
  installLicense,
  removeLicense,
} from '../lib/api'
import type { CommandError, LicensePlan, LicenseRequestSummaryDto } from '../types/license'
import { useLicense } from '../providers/LicenseProvider'
import {
  formatRequestToast,
  getRequestLocationDetails,
  getLicenseBadge,
  getLicenseGateMessage,
  getLicenseSummaryMessage,
  mapCommandErrorToCopy,
  maskDeviceHash,
} from '../lib/license-ui'

const planOptions: { label: string; value: LicensePlan; description: string }[] = [
  { label: 'Mensual', value: 'monthly', description: '30 días' },
  { label: 'Anual', value: 'yearly', description: '365 días' },
  { label: 'Por evento', value: 'per_event', description: '7 días' },
];

type LicensePanelProps = {
  variant?: 'full' | 'gate'
}

export function LicensePanel({ variant = 'full' }: LicensePanelProps) {
  const { status, setStatus, refresh, refreshing } = useLicense()
  const [deviceHash, setDeviceHash] = useState<string>('')
  const [plan, setPlan] = useState<LicensePlan>('monthly')
  const [customerHint, setCustomerHint] = useState('')
  const [lastGeneratedRequest, setLastGeneratedRequest] = useState<LicenseRequestSummaryDto | null>(null)
  const [isGenerating, setIsGenerating] = useState(false)
  const [isInstalling, setIsInstalling] = useState(false)
  const [isRemoving, setIsRemoving] = useState(false)
  const isGateVariant = variant === 'gate'
  const hasInstalledLicense = Boolean(status && !status.is_placeholder)

  useEffect(() => {
    getDeviceHash()
      .then(setDeviceHash)
      .catch((err) => notifyCommandError(err, 'No se pudo obtener el hash del dispositivo'))
  }, [])

  const statusBadge = useMemo(() => getLicenseBadge(status?.status), [status])
  const statusMessage = useMemo(() => getLicenseSummaryMessage(status), [status])
  const maskedDeviceHash = maskDeviceHash(deviceHash)
  const maskedRuntimeHash = maskDeviceHash(status?.device_hash_hex)
  const requestLocationDetails = useMemo(
    () => (lastGeneratedRequest ? getRequestLocationDetails(lastGeneratedRequest) : null),
    [lastGeneratedRequest],
  )
  const exportedRequestPath = requestLocationDetails?.exportedPath ?? ''

  const handleGenerateRequest = async () => {
    const defaultFileName = `license-request-${plan}-${Date.now()}.req`
    const destination = await save({
      title: 'Guardar solicitud de licencia (.req)',
      filters: [{ name: 'License Request', extensions: ['req'] }],
      defaultPath: defaultFileName,
    })
    if (!destination) return

    setIsGenerating(true)
    try {
      const summary = await generateLicenseRequest(
        plan,
        customerHint || undefined,
        destination,
      )
      setLastGeneratedRequest(summary)
      const planLabel = planOptions.find((option) => option.value === plan)?.label ?? plan
      toast.success('Solicitud generada', {
        description: formatRequestToast(summary, { planLabel }),
      })
    } catch (err) {
      notifyCommandError(err, 'No se pudo generar la solicitud')
    } finally {
      setIsGenerating(false)
    }
  }

  const handleRevealRequest = async () => {
    if (!exportedRequestPath) return
    try {
      await revealItemInDir(exportedRequestPath)
    } catch (err) {
      notifyCommandError(err, 'No se pudo abrir la carpeta del archivo')
    }
  }

  const handleCopyRequestPath = async () => {
    if (!exportedRequestPath) return
    try {
      await navigator.clipboard.writeText(exportedRequestPath)
      toast.success('Ruta copiada', { description: exportedRequestPath })
    } catch {
      toast.error('No se pudo copiar la ruta', {
        description: 'Copia manualmente la ubicación mostrada abajo.',
      })
    }
  }

  const handleInstallLicense = async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: 'Licencias', extensions: ['lic'] }],
    })
    if (!selected || Array.isArray(selected)) return

    setIsInstalling(true)
    try {
      const nextStatus = await installLicense({ type: 'path', path: selected })
      setStatus(nextStatus)
      toast.success('Licencia instalada correctamente')
    } catch (err) {
      notifyCommandError(err, 'No se pudo instalar la licencia')
    } finally {
      setIsInstalling(false)
    }
  }

  const handleRemoveLicense = async () => {
    if (!window.confirm('¿Eliminar la licencia instalada?')) return
    setIsRemoving(true)
    try {
      await removeLicense()
      setStatus(null)
      await refresh().catch(() => {})
      toast.success('Licencia eliminada')
    } catch (err) {
      notifyCommandError(err, 'No se pudo eliminar la licencia')
    } finally {
      setIsRemoving(false)
    }
  }

  const handleRefreshStatus = async () => {
    try {
      await refresh()
      toast.success('Estado actualizado')
    } catch (err) {
      notifyCommandError(err, 'No se pudo actualizar el estado')
    }
  }

  return (
    <div className="space-y-6">
      <section className="border border-border rounded-lg p-4 bg-card">
        <div className="flex items-center justify-between flex-wrap gap-3">
          <div>
            <h3 className="text-lg font-medium">Estado de la licencia</h3>
            <p className="text-sm text-muted-foreground">Verifica la validez y detalles de tu licencia</p>
          </div>
          <span className={`px-3 py-1 rounded-full text-sm font-medium ${statusBadge.className}`}>
            {statusBadge.label}
          </span>
        </div>

        <p className="mt-2 text-sm text-muted-foreground">{statusMessage}</p>

        {!isGateVariant && (
          <div className="mt-4 flex flex-wrap gap-3">
            <Button
              variant="outline"
              size="sm"
              onClick={handleRefreshStatus}
              disabled={refreshing}
              className="inline-flex items-center gap-2"
            >
              {refreshing ? <Loader2 className="size-4 animate-spin" /> : <RefreshCcw className="size-4" />}
              {refreshing ? 'Actualizando...' : 'Actualizar estado'}
            </Button>
            <div className="text-xs text-muted-foreground flex items-center gap-2">
              <span className="font-medium">Resultado del guard:</span>
              <span>{getLicenseGateMessage(status?.status)}</span>
            </div>
          </div>
        )}

        {!isGateVariant && hasInstalledLicense && (
          <div className="mt-4 grid grid-cols-1 md:grid-cols-2 gap-4 text-sm">
            <InfoRow label="Cliente" value={status?.customer_name ?? '—'} />
            <InfoRow label="Plan" value={status?.plan ?? '—'} />
            <InfoRow label="Válida hasta" value={status ? formatDate(status.not_after) : '—'} />
            <InfoRow label="License ID" value={status?.license_id ?? '—'} mono />
            <InfoRow label="Device hash" value={maskedRuntimeHash} mono />
          </div>
        )}

        {!isGateVariant && !hasInstalledLicense && (
          <p className="mt-4 text-sm text-muted-foreground">
            Instala una licencia válida para ver los detalles técnicos.
          </p>
        )}

        {!isGateVariant && (
          <div className="mt-4 border-t border-border pt-4">
            <div>
              <div className="text-sm font-medium">Identificador del dispositivo (parcial)</div>
              <div className="font-mono text-xs text-muted-foreground break-all">
                {maskedDeviceHash === '—' ? 'Cargando...' : maskedDeviceHash}
              </div>
            </div>
          </div>
        )}
      </section>

      <section className="border border-border rounded-lg p-4 bg-card space-y-4">
        <div>
          <h3 className="text-lg font-medium">Generar solicitud (.req)</h3>
          <p className="text-sm text-muted-foreground">Crea una solicitud para enviar al generador de licencias.</p>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div>
            <label className="text-sm text-muted-foreground">Plan deseado</label>
            <select
              className="w-full rounded-lg border border-border px-3 py-2 text-sm bg-background"
              value={plan}
              onChange={(e) => setPlan(e.target.value as LicensePlan)}
            >
              {planOptions.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label} — {option.description}
                </option>
              ))}
            </select>
          </div>
          <div>
            <label className="text-sm text-muted-foreground">Nombre del cliente (opcional)</label>
            <Input
              value={customerHint}
              onChange={(e) => setCustomerHint(e.target.value)}
              placeholder="Ej. Rancho El Sol"
            />
          </div>
        </div>

        <Button
          onClick={handleGenerateRequest}
          disabled={isGenerating}
          className="inline-flex items-center gap-2"
        >
          {isGenerating ? <Loader2 className="size-4 animate-spin" /> : <FileDown className="size-4" />}
          {isGenerating ? 'Generando…' : 'Generar solicitud'}
        </Button>

        {lastGeneratedRequest && (
          <div className="rounded-lg border border-emerald-200 bg-emerald-50/70 p-4 space-y-4">
            <div>
              <h4 className="text-sm font-medium text-emerald-900">Solicitud lista para enviar</h4>
              <p className="text-sm text-emerald-900/80">
                Envía este archivo a soporte para generar tu licencia.
              </p>
            </div>

            <div className="grid grid-cols-1 gap-3 text-sm">
              <InfoRow label="Ruta exportada" value={exportedRequestPath} mono />
              {requestLocationDetails?.hasSeparateArchive && requestLocationDetails.archivedPath && (
                <InfoRow label="Copia archivada internamente" value={requestLocationDetails.archivedPath} mono />
              )}
            </div>

            <div className="flex flex-wrap gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={handleRevealRequest}
                className="inline-flex items-center gap-2"
              >
                <FolderOpen className="size-4" />
                Mostrar en carpeta
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={handleCopyRequestPath}
                className="inline-flex items-center gap-2"
              >
                <Copy className="size-4" />
                Copiar ruta
              </Button>
            </div>
          </div>
        )}
      </section>

      <section className="border border-dashed rounded-lg p-4 bg-muted/30 space-y-4">
        <div>
          <h3 className="text-lg font-medium">Instalar o eliminar licencia</h3>
          <p className="text-sm text-muted-foreground">Instala un archivo .lic firmado o elimina la licencia actual.</p>
        </div>
        <div className="flex flex-col sm:flex-row gap-3">
          <Button
            onClick={handleInstallLicense}
            className="flex-1 inline-flex items-center justify-center gap-2"
            disabled={isInstalling}
          >
            {isInstalling ? <Loader2 className="size-4 animate-spin" /> : <Upload className="size-4" />}
            {isInstalling
              ? 'Instalando…'
              : status?.status === 'expired'
              ? 'Instalar nueva licencia'
              : 'Instalar licencia (.lic)'}
          </Button>
          {!isGateVariant && hasInstalledLicense && (
            <Button
              variant="outline"
              onClick={handleRemoveLicense}
              disabled={isRemoving}
              className="inline-flex items-center justify-center gap-2"
            >
              {isRemoving ? <Loader2 className="size-4 animate-spin" /> : <Trash2 className="size-4" />}
              {isRemoving ? 'Eliminando…' : 'Eliminar licencia'}
            </Button>
          )}
        </div>
      </section>
    </div>
  )
}

function InfoRow({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div>
      <div className="text-xs uppercase tracking-wide text-muted-foreground">{label}</div>
      <div className={`text-sm ${mono ? 'font-mono break-all' : ''}`}>{value}</div>
    </div>
  );
}

function formatDate(ts?: number | null) {
  if (!ts) return '—'
  return new Date(ts * 1000).toLocaleString()
}

function notifyCommandError(error: unknown, fallback: string) {
  if (isCommandError(error)) {
    const copy = mapCommandErrorToCopy(error, fallback)
    if (copy.description) {
      toast.error(copy.title, { description: copy.description })
    } else {
      toast.error(copy.title)
    }
    return
  }
  toast.error(fallback, { description: 'Reintenta o contacta a soporte.' })
}

function isCommandError(error: unknown): error is CommandError {
  return Boolean(
    error &&
      typeof error === 'object' &&
      'code' in error &&
      'message' in error,
  )
}
