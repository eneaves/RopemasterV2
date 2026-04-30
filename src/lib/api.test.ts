import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  listTeams,
  getEvents,
  generateLicenseRequest,
  exportEventBackup,
  inspectEventBackup,
  importEventBackup,
} from './api'
import { invoke } from '@tauri-apps/api/core'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

const invokeMock = vi.mocked(invoke)

describe('api wrappers', () => {
  beforeEach(() => {
    invokeMock.mockReset()
  })

  it('listTeams sends camelCase payload expected by Tauri bridge', async () => {
    invokeMock.mockResolvedValueOnce([])
    await listTeams(42)
    expect(invokeMock).toHaveBeenCalledWith('list_teams', { eventId: 42 })
  })

  it('getEvents without seriesId omits payload', async () => {
    invokeMock.mockResolvedValueOnce([])
    await getEvents()
    expect(invokeMock).toHaveBeenCalledWith('list_events')
  })

  it('getEvents with seriesId sends snake_case payload', async () => {
    invokeMock.mockResolvedValueOnce([])
    await getEvents(7)
    expect(invokeMock).toHaveBeenCalledWith('list_events', { seriesId: 7 })
  })

  it('generateLicenseRequest sends camelCase fields expected by the Tauri bridge', async () => {
    invokeMock.mockResolvedValueOnce({
      exported_path: '/tmp/request.req',
      archived_path: '/tmp/archive.req',
      archived_internally: true,
      created_at: 0,
      plan: 'monthly',
      device_hash_hex: 'abcd',
      request_id_hex: 'abcd1234',
      installation_id: 'install-123',
      nonce_hex: 'legacy',
    })
    await generateLicenseRequest('monthly', 'Cliente Demo', '/tmp/request.req')
    expect(invokeMock).toHaveBeenCalledWith('generate_license_request', {
      plan: 'monthly',
      customerNameHint: 'Cliente Demo',
      destinationPath: '/tmp/request.req',
    })
  })

  it('exportEventBackup sends the expected Tauri payload', async () => {
    invokeMock.mockResolvedValueOnce(undefined)
    await exportEventBackup(19, '/tmp/backup.xlsx')
    expect(invokeMock).toHaveBeenCalledWith('export_event_backup', {
      eventId: 19,
      filePath: '/tmp/backup.xlsx',
    })
  })

  it('inspectEventBackup maps snake_case backend fields to camelCase', async () => {
    invokeMock.mockResolvedValueOnce({
      format: 'roping_event_backup',
      version: 1,
      event_name: 'Evento Demo',
      event_date: '2026-04-29',
      rounds: 3,
      ropers_count: 12,
      teams_count: 6,
      runs_count: 18,
      warnings: ['Hoja opcional draw no encontrada'],
    })

    const result = await inspectEventBackup('/tmp/backup.xlsx')

    expect(invokeMock).toHaveBeenCalledWith('inspect_event_backup', {
      filePath: '/tmp/backup.xlsx',
    })
    expect(result).toEqual({
      format: 'roping_event_backup',
      version: 1,
      eventName: 'Evento Demo',
      eventDate: '2026-04-29',
      rounds: 3,
      ropersCount: 12,
      teamsCount: 6,
      runsCount: 18,
      warnings: ['Hoja opcional draw no encontrada'],
    })
  })

  it('importEventBackup sends snake_case payload and maps the response', async () => {
    invokeMock.mockResolvedValueOnce({
      event_id: 55,
      event_name: 'Evento Restaurado',
      ropers_created: 4,
      ropers_reused: 2,
      teams_created: 3,
      runs_created: 9,
      warnings: ['Se recreó el roster desde teams'],
    })

    const result = await importEventBackup({
      filePath: '/tmp/backup.xlsx',
      targetSeriesId: 7,
      restoreStatusMode: 'force_upcoming',
      dedupeRopers: true,
    })

    expect(invokeMock).toHaveBeenCalledWith('import_event_backup', {
      payload: {
        file_path: '/tmp/backup.xlsx',
        target_series_id: 7,
        restore_status_mode: 'force_upcoming',
        dedupe_ropers: true,
      },
    })
    expect(result).toEqual({
      eventId: 55,
      eventName: 'Evento Restaurado',
      ropersCreated: 4,
      ropersReused: 2,
      teamsCreated: 3,
      runsCreated: 9,
      warnings: ['Se recreó el roster desde teams'],
    })
  })
})
