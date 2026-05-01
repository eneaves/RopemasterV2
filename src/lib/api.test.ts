import { beforeEach, describe, expect, it, vi } from 'vitest'
import { listTeams, getEvents, generateLicenseRequest } from './api'
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
})
