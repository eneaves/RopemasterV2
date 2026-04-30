export type ResultsView = 'event' | 'seriesRopers'

export type StandingStatus = 'Calificado' | 'No Time' | 'DQ'

export interface EventStandingRow {
  rank: number
  teamId: number
  headerName: string
  heelerName: string
  totalTime: number | null
  completedRuns: number
  ntCount: number
  dqCount: number
  avgTime: number | null
  bestTime: number | null
  payoff: number
  status: StandingStatus
}

export interface EventResultsSummary {
  qualifiedTeams: number
  cleanRuns: number
  ntCount: number
  dqCount: number
  bestRunTime: number | null
  totalPayout: number
}

export interface EventResultsFiltersState {
  query: string
  place: string
  status: 'Todos' | StandingStatus
}

export type RoperSpecialty = 'header' | 'heeler' | 'both'

export interface SeriesRoperStatRow {
  roperId: number
  roperName: string
  specialty: RoperSpecialty
  eventsPlayed: number
  partnersCount: number
  validRuns: number
  avgTime: number | null
  bestRun: number | null
  wins: number
  podiums: number
  ntCount: number
  dqCount: number
  earnings: number
  rank: number
}

export interface SeriesRopersSummary {
  uniqueRopers: number
  closedEvents: number
  validRuns: number
  totalDistributed: number
  fastestRoperName: string | null
  fastestAvgTime: number | null
  mostWinsRoperName: string | null
  mostWinsCount: number
}

export interface SeriesSummaryPanelItem {
  roperId: number
  name: string
  avgTime: number | null
}

export interface SeriesSummaryPanelData {
  closedEvents: number
  uniqueRopers: number
  teamsRegistered: number
  totalDistributed: number
  avgSeriesTime: number | null
  cleanRunRate: number | null
  topRopers: SeriesSummaryPanelItem[]
}

export interface SeriesRopersFiltersState {
  query: string
  specialty: 'Todos' | RoperSpecialty
  minRuns: '1' | '3' | '5'
  scope: 'Todos' | 'Top 10' | 'Con podio'
}

export interface SeriesRoperHistoryEntry {
  eventId: number
  eventName: string
  partnerName: string
  finishRank: number | null
  totalTime: number | null
  avgTime: number | null
  earnings: number
}

export interface SeriesRoperProfile {
  roperId: number
  roperName: string
  specialty: RoperSpecialty
  rank: number
  avgTime: number | null
  eventsPlayed: number
  wins: number
  podiums: number
  earnings: number
  bestPartnerName: string | null
  bestEventName: string | null
  bestRun: number | null
  history: SeriesRoperHistoryEntry[]
}

export interface TeamRoundRow {
  round: number
  position: number
  timeSec: number | null
  penalty: number
  totalSec: number | null
  noTime: boolean
  dq: boolean
  status: string
}
