import type { BackupInspection, ImportEventBackupResult } from '../types'

const pad2 = (value: number) => String(value).padStart(2, '0')

const sanitizeSegment = (value: string, fallback: string) => {
  const normalized = value
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .replace(/[^a-zA-Z0-9]+/g, '_')
    .replace(/_+/g, '_')
    .replace(/^_+|_+$/g, '')

  return normalized || fallback
}

export function buildEventBackupFilename(seriesName: string, eventName: string, now = new Date()) {
  const year = now.getFullYear()
  const month = pad2(now.getMonth() + 1)
  const day = pad2(now.getDate())
  const hour = pad2(now.getHours())
  const minute = pad2(now.getMinutes())
  const safeSeries = sanitizeSegment(seriesName, 'Serie')
  const safeEvent = sanitizeSegment(eventName, 'Evento')
  return `BACKUP_${safeSeries}_${safeEvent}_${year}${month}${day}_${hour}${minute}.xlsx`
}

export function getEventBackupErrorMessage(error: unknown, fallback: string) {
  const message = String(error ?? '')
  if (message.includes('BACKUP_INVALID_FORMAT')) return 'El archivo no es un backup válido de Roping Manager.'
  if (message.includes('BACKUP_UNSUPPORTED_VERSION')) return 'La versión del backup no es compatible con esta app.'
  if (message.includes('BACKUP_MISSING_SHEET')) return 'Al backup le falta una hoja requerida.'
  if (message.includes('BACKUP_MISSING_COLUMN')) return 'Al backup le faltan columnas requeridas.'
  if (message.includes('BACKUP_DUPLICATE_KEY')) return 'El backup contiene claves duplicadas y no se puede restaurar.'
  if (message.includes('BACKUP_BROKEN_REFERENCE')) return 'El backup tiene referencias internas inválidas entre hojas.'
  if (message.includes('BACKUP_INVALID_VALUE')) return 'El backup contiene valores inválidos.'
  if (message.includes('BACKUP_IMPORT_FAILED')) return 'La importación falló y no se guardó ningún cambio.'
  return fallback
}

export function getBackupPreviewRows(inspection: BackupInspection) {
  return [
    { label: 'Nombre', value: inspection.eventName },
    { label: 'Fecha', value: inspection.eventDate },
    { label: 'Rondas', value: String(inspection.rounds) },
    { label: 'Ropers', value: String(inspection.ropersCount) },
    { label: 'Equipos', value: String(inspection.teamsCount) },
    { label: 'Runs', value: String(inspection.runsCount) },
    { label: 'Versión', value: `v${inspection.version}` },
  ]
}

export function canConfirmBackupImport(args: {
  filePath: string
  inspection: BackupInspection | null
  targetSeriesId: number | null
  isImporting: boolean
}) {
  return Boolean(args.filePath && args.inspection && args.targetSeriesId && !args.isImporting)
}

export function buildImportSuccessDescription(
  result: ImportEventBackupResult,
  targetSeriesName?: string | null
) {
  const base = `${result.eventName} restaurado como evento nuevo`
  if (targetSeriesName) {
    return `${base} en ${targetSeriesName}. Equipos: ${result.teamsCreated} | Runs: ${result.runsCreated}`
  }
  return `${base}. Equipos: ${result.teamsCreated} | Runs: ${result.runsCreated}`
}
