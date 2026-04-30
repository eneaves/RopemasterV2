import { describe, expect, it } from 'vitest'
import {
  buildEventBackupFilename,
  getEventBackupErrorMessage,
  getBackupPreviewRows,
  canConfirmBackupImport,
  buildImportSuccessDescription,
} from './event-backup-ui'

describe('event backup ui helpers', () => {
  it('builds the requested backup filename format', () => {
    const filename = buildEventBackupFilename(
      'Serie Élite 2026',
      'Evento Final #1',
      new Date('2026-04-29T08:05:00')
    )

    expect(filename).toBe('BACKUP_Serie_Elite_2026_Evento_Final_1_20260429_0805.xlsx')
  })

  it('formats preview rows for the import confirmation', () => {
    const rows = getBackupPreviewRows({
      format: 'roping_event_backup',
      version: 1,
      eventName: 'Backup Demo',
      eventDate: '2026-04-29',
      rounds: 4,
      ropersCount: 20,
      teamsCount: 10,
      runsCount: 40,
      warnings: ['Preview warning'],
    })

    expect(rows).toEqual([
      { label: 'Nombre', value: 'Backup Demo' },
      { label: 'Fecha', value: '2026-04-29' },
      { label: 'Rondas', value: '4' },
      { label: 'Ropers', value: '20' },
      { label: 'Equipos', value: '10' },
      { label: 'Runs', value: '40' },
      { label: 'Versión', value: 'v1' },
    ])
  })

  it('maps inspect/import backend codes to friendly UI errors', () => {
    expect(getEventBackupErrorMessage('BACKUP_INVALID_FORMAT: bad file', 'fallback')).toBe(
      'El archivo no es un backup válido de Roping Manager.'
    )
    expect(getEventBackupErrorMessage('BACKUP_MISSING_COLUMN: runs.total_sec', 'fallback')).toBe(
      'Al backup le faltan columnas requeridas.'
    )
    expect(getEventBackupErrorMessage('unknown failure', 'fallback')).toBe('fallback')
  })

  it('only enables import confirmation when the preview is complete', () => {
    const inspection = {
      format: 'roping_event_backup',
      version: 1,
      eventName: 'Backup Demo',
      eventDate: '2026-04-29',
      rounds: 3,
      ropersCount: 8,
      teamsCount: 4,
      runsCount: 12,
      warnings: [],
    }

    expect(
      canConfirmBackupImport({
        filePath: '/tmp/demo.xlsx',
        inspection,
        targetSeriesId: 3,
        isImporting: false,
      })
    ).toBe(true)

    expect(
      canConfirmBackupImport({
        filePath: '',
        inspection,
        targetSeriesId: 3,
        isImporting: false,
      })
    ).toBe(false)
  })

  it('builds a success message without leaking internal paths', () => {
    const description = buildImportSuccessDescription(
      {
        eventId: 11,
        eventName: 'Backup Final',
        ropersCreated: 3,
        ropersReused: 2,
        teamsCreated: 5,
        runsCreated: 15,
        warnings: ['warning'],
      },
      'Serie Norte'
    )

    expect(description).toBe(
      'Backup Final restaurado como evento nuevo en Serie Norte. Equipos: 5 | Runs: 15'
    )
  })
})
