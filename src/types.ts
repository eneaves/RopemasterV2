export interface Series {
  id: number
  name: string
  season: string
  status: 'active' | 'upcoming' | 'archived'
  startDate?: string | null
  endDate?: string | null
  createdAt?: string
  updatedAt?: string
  // Frontend computed or optional
  dateRange?: string
  eventsCount?: number
  progress?: number
  description?: string
}

export interface Event {
  id: number
  seriesId: number
  name: string
  date: string
  status: 'draft' | 'upcoming' | 'active' | 'locked' | 'completed' | 'finalized' | 'archived' | 'inactive'
  rounds: number
  location?: string | null
  entryFee?: number | null
  prizePool?: number | null
  maxTeamRating?: number | null
  teamsCount: number
  pot: number
  payoffAllocation?: string | null
  adminPin?: string | null
  createdAt?: string
  updatedAt?: string
  // Legacy/Frontend computed
  lastUpdated?: string
}

export interface Team {
  id: string | number
  teamId?: number
  header: string
  heeler: string
  rating?: number
}

export interface Run {
  id: string
  teamId: number
  team: Team
  round: number
  position: number
  time: number | null
  penalty: number
  noTime: boolean
  dq: boolean
  status: 'pending' | 'completed' | 'skipped'
}

export interface Standing {
  position: number
  team: Team
  roundsCompleted: number
  totalTime: number | null
  average: number | null
  status: 'qualified' | 'warning' | 'eliminated'
}

export interface Payoff {
  id: string
  eventId: string
  position: number
  amount: number
  teamId?: string
  distributed?: boolean
}

export interface AuditLogItem {
  id: number
  action: string
  entity_type: string
  entity_id?: number
  user_id?: number
  metadata?: string
  created_at: string
}

export interface SeriesLog {
  id: number
  seriesId: number
  eventId?: number | null
  action: string
  details: string
  timestamp: string
}

export interface DashboardStats {
  total_series: number
  active_series: number
  total_events: number
  active_events: number
  completed_events: number
  upcoming_events: number
  locked_events: number
  total_teams: number
  total_pot: number
  upcoming_events_30d: number
  global_progress: number
}
