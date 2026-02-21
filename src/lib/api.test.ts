import { beforeEach, describe, expect, it, vi } from 'vitest'
import { listTeams, getEvents } from './api'
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
})
