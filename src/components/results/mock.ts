import type { Event } from '../../types'
import type {
  EventResultsSummary,
  EventStandingRow,
  RoperSpecialty,
  SeriesRoperProfile,
  SeriesRoperStatRow,
  SeriesRopersSummary,
  SeriesSummaryPanelData,
} from './types'

type MockBundle = {
  summary: SeriesRopersSummary
  panel: SeriesSummaryPanelData
  rows: SeriesRoperStatRow[]
  isMock: boolean
}

function hashName(input: string) {
  let hash = 0
  for (let i = 0; i < input.length; i += 1) {
    hash = (hash * 31 + input.charCodeAt(i)) | 0
  }
  return Math.abs(hash)
}

function getEventStatus(event?: Event | null) {
  return (event as any)?.status ?? ''
}

function getEventName(event?: Event | null) {
  return (event as any)?.name ?? 'Evento'
}

function getEventId(event?: Event | null) {
  return Number((event as any)?.id ?? 0)
}

function getEventPot(event?: Event | null) {
  const raw = (event as any)?.pot ?? (event as any)?.pot_amount ?? 0
  return Number(raw) || 0
}

function getEventTeamsCount(event?: Event | null) {
  const raw = (event as any)?.teamsCount ?? (event as any)?.teams_count ?? 0
  return Number(raw) || 0
}

function isClosedEvent(event?: Event | null) {
  const status = getEventStatus(event)
  return status === 'completed' || status === 'locked'
}

type AggregateRow = {
  roperId: number
  roperName: string
  specialty: RoperSpecialty
  eventsPlayed: number
  partners: Set<string>
  validRuns: number
  totalWeightedTime: number
  bestRun: number | null
  wins: number
  podiums: number
  ntCount: number
  dqCount: number
  earnings: number
}

function upsertAggregate(
  bucket: Map<string, AggregateRow>,
  role: 'header' | 'heeler',
  row: EventStandingRow,
) {
  const roperName = role === 'header' ? row.headerName : row.heelerName
  const partnerName = role === 'header' ? row.heelerName : row.headerName
  const key = `${role}:${roperName.toLowerCase()}`
  const existing = bucket.get(key)

  if (!existing) {
    bucket.set(key, {
      roperId: hashName(key),
      roperName,
      specialty: role,
      eventsPlayed: 1,
      partners: new Set([partnerName]),
      validRuns: row.completedRuns,
      totalWeightedTime: (row.avgTime ?? 0) * row.completedRuns,
      bestRun: row.bestTime,
      wins: row.rank === 1 ? 1 : 0,
      podiums: row.rank <= 3 ? 1 : 0,
      ntCount: row.ntCount,
      dqCount: row.dqCount,
      earnings: row.payoff / 2,
    })
    return
  }

  existing.partners.add(partnerName)
  existing.validRuns += row.completedRuns
  existing.totalWeightedTime += (row.avgTime ?? 0) * row.completedRuns
  existing.bestRun =
    existing.bestRun === null
      ? row.bestTime
      : row.bestTime === null
        ? existing.bestRun
        : Math.min(existing.bestRun, row.bestTime)
  existing.wins += row.rank === 1 ? 1 : 0
  existing.podiums += row.rank <= 3 ? 1 : 0
  existing.ntCount += row.ntCount
  existing.dqCount += row.dqCount
  existing.earnings += row.payoff / 2
}

export function buildEventResultsSummary(rows: EventStandingRow[]): EventResultsSummary {
  return {
    qualifiedTeams: rows.filter((row) => row.status === 'Calificado').length,
    cleanRuns: rows.reduce((acc, row) => acc + row.completedRuns, 0),
    ntCount: rows.reduce((acc, row) => acc + row.ntCount, 0),
    dqCount: rows.reduce((acc, row) => acc + row.dqCount, 0),
    bestRunTime: rows.reduce<number | null>((best, row) => {
      if (row.bestTime === null) return best
      if (best === null) return row.bestTime
      return Math.min(best, row.bestTime)
    }, null),
    totalPayout: rows.reduce((acc, row) => acc + row.payoff, 0),
  }
}

export function buildSeriesRopersMock(
  rows: EventStandingRow[],
  selectedEvent: Event | undefined,
  eventsList: Event[],
): MockBundle {
  const closedEvents = eventsList.filter((event) => isClosedEvent(event)).length
  const teamsRegistered = eventsList
    .filter((event) => isClosedEvent(event))
    .reduce((acc, event) => acc + getEventTeamsCount(event), 0)

  const bucket = new Map<string, AggregateRow>()
  rows.forEach((row) => {
    upsertAggregate(bucket, 'header', row)
    upsertAggregate(bucket, 'heeler', row)
  })

  const aggregates = Array.from(bucket.values())
  const ranked = aggregates
    .map<SeriesRoperStatRow>((aggregate) => ({
      roperId: aggregate.roperId,
      roperName: aggregate.roperName,
      specialty: aggregate.specialty,
      eventsPlayed: aggregate.eventsPlayed,
      partnersCount: aggregate.partners.size,
      validRuns: aggregate.validRuns,
      avgTime:
        aggregate.validRuns > 0 ? aggregate.totalWeightedTime / aggregate.validRuns : null,
      bestRun: aggregate.bestRun,
      wins: aggregate.wins,
      podiums: aggregate.podiums,
      ntCount: aggregate.ntCount,
      dqCount: aggregate.dqCount,
      earnings: aggregate.earnings,
      rank: 0,
    }))
    .sort((a, b) => {
      if (a.avgTime !== null && b.avgTime !== null && a.avgTime !== b.avgTime) {
        return a.avgTime - b.avgTime
      }
      if (a.avgTime !== null && b.avgTime === null) return -1
      if (a.avgTime === null && b.avgTime !== null) return 1
      if (a.wins !== b.wins) return b.wins - a.wins
      if (a.podiums !== b.podiums) return b.podiums - a.podiums
      if (a.bestRun !== null && b.bestRun !== null && a.bestRun !== b.bestRun) {
        return a.bestRun - b.bestRun
      }
      if (a.bestRun !== null && b.bestRun === null) return -1
      if (a.bestRun === null && b.bestRun !== null) return 1
      return b.earnings - a.earnings
    })
    .map((row, index) => ({ ...row, rank: index + 1 }))

  const validRuns = ranked.reduce((acc, row) => acc + row.validRuns, 0)
  const attempts = ranked.reduce((acc, row) => acc + row.validRuns + row.ntCount + row.dqCount, 0)
  const totalDistributed = rows.reduce((acc, row) => acc + row.payoff, 0)
  const fastest = ranked.find((row) => row.avgTime !== null) ?? null
  const mostWins = ranked.reduce<SeriesRoperStatRow | null>((current, row) => {
    if (current === null) return row
    if (row.wins > current.wins) return row
    return current
  }, null)

  const avgSeriesTime =
    ranked.filter((row) => row.avgTime !== null).length > 0
      ? ranked
          .filter((row) => row.avgTime !== null)
          .reduce((acc, row) => acc + (row.avgTime ?? 0), 0) /
        ranked.filter((row) => row.avgTime !== null).length
      : null

  return {
    isMock: true,
    summary: {
      uniqueRopers: ranked.length,
      closedEvents,
      validRuns,
      totalDistributed,
      fastestRoperName: fastest?.roperName ?? null,
      fastestAvgTime: fastest?.avgTime ?? null,
      mostWinsRoperName: mostWins?.roperName ?? null,
      mostWinsCount: mostWins?.wins ?? 0,
    },
    panel: {
      closedEvents,
      uniqueRopers: ranked.length,
      teamsRegistered,
      totalDistributed: totalDistributed || getEventPot(selectedEvent),
      avgSeriesTime,
      cleanRunRate: attempts > 0 ? (validRuns / attempts) * 100 : null,
      topRopers: ranked.slice(0, 5).map((row) => ({
        roperId: row.roperId,
        name: row.roperName,
        avgTime: row.avgTime,
      })),
    },
    rows: ranked,
  }
}

export function buildMockRoperProfile(
  roperId: number | null,
  rows: SeriesRoperStatRow[],
  selectedEvent: Event | undefined,
): SeriesRoperProfile | null {
  if (roperId === null) return null
  const roper = rows.find((row) => row.roperId === roperId)
  if (!roper) return null

  const partnerName = selectedEvent ? 'Compañero del evento actual' : 'Compañero'
  return {
    roperId: roper.roperId,
    roperName: roper.roperName,
    specialty: roper.specialty,
    rank: roper.rank,
    avgTime: roper.avgTime,
    eventsPlayed: roper.eventsPlayed,
    wins: roper.wins,
    podiums: roper.podiums,
    earnings: roper.earnings,
    bestPartnerName: partnerName,
    bestEventName: selectedEvent ? getEventName(selectedEvent) : null,
    bestRun: roper.bestRun,
    history: selectedEvent
      ? [
          {
            eventId: getEventId(selectedEvent),
            eventName: getEventName(selectedEvent),
            partnerName,
            finishRank: roper.rank,
            totalTime: roper.avgTime !== null ? roper.avgTime * roper.validRuns : null,
            avgTime: roper.avgTime,
            earnings: roper.earnings,
          },
        ]
      : [],
  }
}
