import { useEffect, useMemo, useState } from 'react';
import { open, save } from '@tauri-apps/plugin-dialog';
import { toast } from 'sonner';

import { Button } from './ui/button';
import { Input } from './ui/input';
import { Textarea } from './ui/textarea';

import {
  getDeviceHash,
  generateLicenseRequest,
  installLicense,
  removeLicense,
} from '../lib/api';
import type {
  CommandError,
  LicensePlan,
} from '../types/license';
import { useLicense } from '../providers/LicenseProvider';

const planOptions: { label: string; value: LicensePlan; description: string }[] = [
  { label: 'Mensual', value: 'monthly', description: '30 días' },
  { label: 'Anual', value: 'yearly', description: '365 días' },
  { label: 'Por evento', value: 'per_event', description: '7 días' },
];

export function LicensePanel() {
  const { status, setStatus } = useLicense();
  const [deviceHash, setDeviceHash] = useState<string>('');
  const [plan, setPlan] = useState<LicensePlan>('monthly');
  const [customerHint, setCustomerHint] = useState('');
  const [isBusy, setIsBusy] = useState(false);

  useEffect(() => {
    getDeviceHash()
      .then(setDeviceHash)
      .catch((err) => handleCommandError(err, 'No se pudo obtener el hash del dispositivo'));
  }, []);

  const statusBadge = useMemo(() => {
    switch (status?.status) {
      case 'active':
        return { label: 'Activa', className: 'bg-green-50 text-green-700' };
      case 'expired':
        return { label: 'Expirada', className: 'bg-red-50 text-red-700' };
      case 'not_yet_valid':
        return { label: 'Pendiente', className: 'bg-yellow-50 text-yellow-700' };
      case 'invalid_device':
        return { label: 'Otro dispositivo', className: 'bg-orange-50 text-orange-700' };
      default:
        return { label: 'No instalada', className: 'bg-slate-100 text-slate-600' };
    }
  }, [status]);

  const statusMessage = useMemo(() => {
    if (!status) {
      return 'Instala una licencia válida para activar Roping Manager.';
    }
    const expiresAt = formatDate(status.not_after);
    const startsAt = formatDate(status.not_before);
    switch (status.status) {
      case 'active':
        return `Licencia activa. Expira el ${expiresAt}.`;
      case 'expired':
        return `Licencia expirada desde ${expiresAt}. Instala una licencia nueva para continuar.`;
      case 'not_yet_valid':
        return `La licencia aún no es válida. Revisa la fecha/hora del sistema (disponible desde ${startsAt}).`;
      case 'invalid_device':
        return `La licencia instalada pertenece a otro dispositivo (hash ${status.device_hash_hex}).`;
      default:
        return '';
    }
  }, [status]);

  const handleCopyHash = async () => {
    if (!deviceHash) return;
    await navigator.clipboard.writeText(deviceHash);
    toast.success('Hash copiado');
  };

  const handleGenerateRequest = async () => {
    const defaultFileName = `license-request-${plan}-${Date.now()}.req`;
    const destination = await save({
      title: 'Guardar solicitud de licencia (.req)',
      filters: [{ name: 'License Request', extensions: ['req'] }],
      defaultPath: defaultFileName,
    });
    if (!destination) return;

    setIsBusy(true);
    try {
      const summary = await generateLicenseRequest(
        plan,
        customerHint || undefined,
        destination,
      );
      toast.success('Solicitud generada', {
        description: summary.path,
      });
    } catch (err) {
      handleCommandError(err, 'No se pudo generar la solicitud');
    } finally {
      setIsBusy(false);
    }
  };

  const handleInstallLicense = async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: 'Licencias', extensions: ['lic'] }],
    });
    if (!selected || Array.isArray(selected)) return;

    setIsBusy(true);
    try {
      const nextStatus = await installLicense({ type: 'path', path: selected });
      setStatus(nextStatus);
      toast.success('Licencia instalada correctamente');
    } catch (err) {
      handleCommandError(err, 'No se pudo instalar la licencia');
    } finally {
      setIsBusy(false);
    }
  };

  const handleRemoveLicense = async () => {
    if (!window.confirm('¿Eliminar la licencia instalada?')) return;
    setIsBusy(true);
    try {
      await removeLicense();
      setStatus(null);
      toast.success('Licencia eliminada');
    } catch (err) {
      handleCommandError(err, 'No se pudo eliminar la licencia');
    } finally {
      setIsBusy(false);
    }
  };

  return (
    <div className="space-y-6">
      <section className="border border-border rounded-lg p-4 bg-card">
        <div className="flex items-center justify-between">
          <div>
            <h3 className="text-lg font-medium">Estado de la licencia</h3>
            <p className="text-sm text-muted-foreground">Verifica la validez y detalles de tu licencia</p>
          </div>
          <span className={`px-3 py-1 rounded-full text-sm font-medium ${statusBadge.className}`}>
            {statusBadge.label}
          </span>
        </div>

        <p className="mt-2 text-sm text-muted-foreground">{statusMessage}</p>

        <div className="mt-4 grid grid-cols-1 md:grid-cols-2 gap-4 text-sm">
          <InfoRow label="Cliente" value={status?.customer_name ?? '—'} />
          <InfoRow label="Plan" value={status?.plan ?? '—'} />
          <InfoRow label="License ID" value={status?.license_id ?? '—'} mono />
          <InfoRow
            label="Válida hasta"
            value={status ? formatDate(status.not_after) : '—'}
          />
        </div>

        <div className="mt-4 border-t border-border pt-4">
          <div className="flex items-center justify-between">
            <div>
              <div className="text-sm font-medium">Device hash</div>
              <div className="font-mono text-xs text-muted-foreground break-all">{deviceHash || 'Cargando...'}</div>
            </div>
            <Button variant="outline" size="sm" onClick={handleCopyHash} disabled={!deviceHash}>
              Copiar
            </Button>
          </div>
        </div>
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

        <Button onClick={handleGenerateRequest} disabled={isBusy}>
          Generar solicitud
        </Button>
      </section>

      <section className="border border-dashed rounded-lg p-4 bg-muted/30 space-y-4">
        <div>
          <h3 className="text-lg font-medium">Instalar o eliminar licencia</h3>
          <p className="text-sm text-muted-foreground">Instala un archivo .lic firmado o elimina la licencia actual.</p>
        </div>
        <div className="flex flex-col sm:flex-row gap-3">
          <Button onClick={handleInstallLicense} className="flex-1" disabled={isBusy}>
            {status?.status === 'expired' ? 'Instalar nueva licencia' : 'Instalar licencia (.lic)'}
          </Button>
          <Button variant="outline" onClick={handleRemoveLicense} disabled={isBusy || !status}>
            Eliminar licencia
          </Button>
        </div>
      </section>

      {status && (
        <section className="border border-border rounded-lg p-4 bg-card space-y-3">
          <h3 className="text-lg font-medium">Detalles técnicos</h3>
          <Textarea
            className="h-32 font-mono text-xs"
            readOnly
            value={JSON.stringify(status, null, 2)}
          />
        </section>
      )}
    </div>
  );
}

function InfoRow({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div>
      <div className="text-xs uppercase tracking-wide text-muted-foreground">{label}</div>
      <div className={`text-sm ${mono ? 'font-mono break-all' : ''}`}>{value}</div>
    </div>
  );
}

function formatDate(ts: number) {
  return new Date(ts * 1000).toLocaleString();
}

function handleCommandError(error: unknown, fallback: string) {
  if (isCommandError(error)) {
    toast.error(error.message, { description: error.code });
  } else if (error instanceof Error) {
    toast.error(fallback, { description: error.message });
  } else {
    toast.error(fallback);
  }
}

function isCommandError(error: unknown): error is CommandError {
  return Boolean(
    error &&
    typeof error === 'object' &&
    'code' in error &&
    'message' in error
  );
}
