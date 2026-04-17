import type {
  CommandError,
  LicenseRequestSummaryDto,
  LicenseStatusDto,
  LicenseUiState,
} from '../types/license'

type BadgeCopy = { label: string; className: string }
type ErrorCopy = { title: string; description?: string }

const DEFAULT_BADGE: BadgeCopy = { label: 'Sin licencia', className: 'bg-slate-100 text-slate-600' }

const BADGE_MAP: Record<LicenseUiState, BadgeCopy> = {
  active: { label: 'Activa', className: 'bg-green-50 text-green-700' },
  expired: { label: 'Expirada', className: 'bg-red-50 text-red-700' },
  not_yet_valid: { label: 'Pendiente', className: 'bg-yellow-50 text-yellow-700' },
  device_mismatch: { label: 'Otro dispositivo', className: 'bg-orange-50 text-orange-700' },
  missing: { label: 'Sin licencia', className: 'bg-slate-100 text-slate-600' },
  invalid: { label: 'Licencia inválida', className: 'bg-rose-50 text-rose-700' },
}

const STATUS_MESSAGES: Partial<Record<LicenseUiState, (status: LicenseStatusDto) => string>> = {
  active: (status) => `Licencia activa. Expira el ${formatDate(status.not_after)}.`,
  expired: (status) => `Licencia expirada desde ${formatDate(status.not_after)}. Instala una nueva para continuar.`,
  not_yet_valid: (status) =>
    `La licencia será válida a partir del ${formatDate(status.not_before)}. Verifica la fecha/hora del sistema.`,
  device_mismatch: () =>
    'La licencia instalada pertenece a otro dispositivo autorizado. Solicita una nueva para este equipo.',
  invalid: () => 'El archivo de licencia es inválido o está corrupto. Importa una licencia valida emitida por el generador.',
  missing: () => 'Instala una licencia válida para desbloquear todas las funciones.',
}

const GATE_MESSAGES: Record<LicenseUiState, string> = {
  active: 'Licencia verificada correctamente.',
  expired: 'La licencia instalada ha expirado. Instala una nueva para desbloquear las funciones.',
  not_yet_valid: 'La licencia aún no es válida en este dispositivo. Verifica la fecha u obtén asistencia.',
  device_mismatch: 'La licencia pertenece a otro dispositivo. Solicita una licencia para este equipo.',
  invalid: 'La licencia persistida es inválida o está corrupta. Importa un archivo .lic válido.',
  missing: 'No se detectó una licencia instalada. Las funciones permanecerán bloqueadas hasta instalar una.',
}

const ERROR_COPY: Record<string, ErrorCopy> = {
  LicenseRequired: {
    title: 'Licencia requerida',
    description: 'Instala una licencia válida para continuar.',
  },
  Expired: {
    title: 'Licencia expirada',
    description: 'Solicita una licencia vigente o renueva la actual.',
  },
  NotYetValid: {
    title: 'Licencia aún no válida',
    description: 'Verifica la fecha/hora del sistema o la ventana de vigencia.',
  },
  DeviceMismatch: {
    title: 'Licencia de otro dispositivo',
    description: 'El archivo pertenece a otra instalación.',
  },
  Invalid: {
    title: 'Licencia inválida',
    description: 'El archivo está dañado o no coincide con el runtime.',
  },
  SignatureFailed: {
    title: 'Firma inválida',
    description: 'El archivo .lic no supera la verificación criptográfica.',
  },
  Parse: {
    title: 'Archivo inválido',
    description: 'El archivo no tiene el formato esperado.',
  },
  Io: {
    title: 'Error de lectura/escritura',
    description: 'Verifica permisos de disco o vuelve a intentar.',
  },
  AppIdMismatch: {
    title: 'Licencia incompatible',
    description: 'El archivo no corresponde a esta aplicación.',
  },
}

export function getLicenseBadge(state?: LicenseUiState | null): BadgeCopy {
  if (!state) return DEFAULT_BADGE
  return BADGE_MAP[state] ?? DEFAULT_BADGE
}

export function getLicenseSummaryMessage(status: LicenseStatusDto | null): string {
  if (!status) {
    return 'Instala una licencia válida para activar Roping Manager.'
  }
  const formatter = STATUS_MESSAGES[status.status]
  if (formatter) {
    return formatter(status)
  }
  return STATUS_MESSAGES.missing?.(status) ?? 'Instala una licencia válida para continuar.'
}

export function getLicenseGateMessage(state?: LicenseUiState | null): string {
  if (!state) return GATE_MESSAGES.missing
  return GATE_MESSAGES[state] ?? GATE_MESSAGES.missing
}

export function mapCommandErrorToCopy(error: CommandError | null, fallback: string): ErrorCopy {
  if (!error) {
    return { title: fallback }
  }
  const mapping = ERROR_COPY[error.code]
  if (mapping) {
    return {
      title: mapping.title,
      description: mapping.description,
    }
  }
  return {
    title: fallback,
    description: 'Intenta de nuevo o contacta a soporte.',
  }
}

function formatDate(value?: number | null) {
  if (!value) return '—'
  return new Date(value * 1000).toLocaleString('es-MX', {
    dateStyle: 'medium',
    timeStyle: 'short',
  })
}

export function maskIdentifier(
  value?: string | null,
  opts: { prefix?: number; suffix?: number } = {},
): string {
  const trimmed = value?.trim()
  if (!trimmed) return '—'
  const prefix = Math.max(1, opts.prefix ?? 4)
  const suffix = Math.max(1, opts.suffix ?? 4)
  if (trimmed.length <= prefix + suffix) {
    return '••••'
  }
  return `${trimmed.slice(0, prefix)}••••${trimmed.slice(-suffix)}`
}

export function maskDeviceHash(value?: string | null): string {
  return maskIdentifier(value, { prefix: 4, suffix: 4 })
}

export function formatRequestToast(
  summary: Pick<
    LicenseRequestSummaryDto,
    'plan' | 'exported_path' | 'archived_path' | 'request_id_hex' | 'installation_id'
  >,
  options?: { planLabel?: string },
): string {
  const details = getRequestLocationDetails(summary)
  const planLabel = options?.planLabel ?? summary.plan
  const parts = [
    `Plan: ${planLabel}`,
    `Exportado en: ${details.exportedPath}`,
    `Solicitud: ${maskIdentifier(summary.request_id_hex)}`,
  ]
  if (summary.installation_id) {
    parts.push(`Instalación: ${maskIdentifier(summary.installation_id, { prefix: 4, suffix: 4 })}`)
  }
  return parts.join(' • ')
}

export function getRequestLocationDetails(
  summary: Pick<LicenseRequestSummaryDto, 'exported_path' | 'archived_path'>,
) {
  return {
    exportedPath: summary.exported_path,
    archivedPath: summary.archived_path ?? null,
    hasSeparateArchive: Boolean(summary.archived_path),
  }
}
