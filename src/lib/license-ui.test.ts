import { describe, expect, it } from 'vitest'
import {
  formatRequestToast,
  getRequestLocationDetails,
  getLicenseBadge,
  getLicenseGateMessage,
  getLicenseSummaryMessage,
  mapCommandErrorToCopy,
  maskDeviceHash,
  maskIdentifier,
} from './license-ui'
import type { LicenseRequestSummaryDto, LicenseStatusDto } from '../types/license'

const baseStatus: LicenseStatusDto = {
  status: 'active',
  plan: 'monthly',
  customer_name: 'QA',
  license_id: 'LIC-TEST',
  not_before: 1_700_000_000,
  not_after: 1_800_000_000,
  max_clock_skew: 60,
  device_hash_hex: 'abcdef123456',
  installed_at: 1_700_000_000,
  last_verified_at: 1_700_000_100,
  last_checked_at: 1_700_000_200,
  is_placeholder: false,
}

describe('license-ui helpers', () => {
  it('returns distinct badges per status', () => {
    const states: LicenseStatusDto['status'][] = [
      'active',
      'expired',
      'not_yet_valid',
      'device_mismatch',
      'missing',
      'invalid',
    ]
    const labels = states.map((state) => getLicenseBadge(state).label)
    expect(new Set(labels).size).toBe(states.length)
  })

  it('returns contextual summary messages', () => {
    expect(getLicenseSummaryMessage(null)).toMatch(/Instala una licencia/)
    expect(getLicenseSummaryMessage({ ...baseStatus, status: 'active' })).toMatch(/Licencia activa/)
    expect(getLicenseSummaryMessage({ ...baseStatus, status: 'expired' })).toMatch(/expirada/i)
    expect(getLicenseSummaryMessage({ ...baseStatus, status: 'device_mismatch' })).toMatch(/pertenece a otro dispositivo/i)
  })

  it('maps each status to a gate message', () => {
    const states: (LicenseStatusDto['status'] | null)[] = [
      null,
      'missing',
      'invalid',
      'expired',
      'not_yet_valid',
      'device_mismatch',
      'active',
    ]
    states.forEach((state) => {
      const message = getLicenseGateMessage(state ?? undefined)
      expect(typeof message).toBe('string')
      expect(message.length).toBeGreaterThan(5)
    })
  })

  it('maps command errors to friendly titles', () => {
    const expired = mapCommandErrorToCopy({ code: 'Expired', message: 'Expired license' }, 'Fallback')
    expect(expired.title).toBe('Licencia expirada')
    expect(expired.description).toBe('Solicita una licencia vigente o renueva la actual.')

    const unknown = mapCommandErrorToCopy({ code: 'Random', message: '???' }, 'Operación fallida')
    expect(unknown.title).toBe('Operación fallida')
    expect(unknown.description).toBe('Intenta de nuevo o contacta a soporte.')
  })

  it('masks identifiers consistently', () => {
    expect(maskIdentifier('abcd1234wxyz5678')).toBe('abcd••••5678')
    expect(maskIdentifier('short')).toBe('••••')
    expect(maskDeviceHash('0011223344556677')).toBe('0011••••6677')
  })

  it('formats request toast without leaking full identifiers', () => {
    const summary: LicenseRequestSummaryDto = {
      exported_path: '/Users/demo/Desktop/request.req',
      archived_path: '/tmp/archive.req',
      archived_internally: true,
      created_at: 0,
      plan: 'monthly',
      device_hash_hex: 'abcd',
      request_id_hex: 'abcd1234wxyz5678',
      installation_id: 'install-abcdef123456',
    }
    const formatted = formatRequestToast(summary, { planLabel: 'Mensual' })
    expect(formatted).toContain('Plan: Mensual')
    expect(formatted).toContain('Exportado en: /Users/demo/Desktop/request.req')
    expect(formatted).toContain('Solicitud: abcd••••5678')
    expect(formatted).toContain('Instalación: inst••••3456')
    expect(formatted).not.toMatch(summary.request_id_hex)
    expect(formatted).not.toMatch(summary.installation_id)
  })

  it('distinguishes exported and archived request paths for the UI', () => {
    const details = getRequestLocationDetails({
      exported_path: '/Users/demo/Desktop/request.req',
      archived_path: '/tmp/internal/archive.req',
    })

    expect(details.exportedPath).toBe('/Users/demo/Desktop/request.req')
    expect(details.archivedPath).toBe('/tmp/internal/archive.req')
    expect(details.hasSeparateArchive).toBe(true)
  })
})
