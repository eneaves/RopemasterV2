import { invoke } from '@tauri-apps/api/core';

// Series
export const getSeries = () => invoke<any[]>('list_series');

export const createSeries = (payload: {
  name: string;
  season: string;
  status: 'active' | 'upcoming' | 'archived';
  start_date?: string | null;
  end_date?: string | null;
}) => invoke<number>('create_series', { payload });

// Teams
export const listTeams = async (eventId: number) => {
  try {
    // eslint-disable-next-line no-console
    console.debug('[api] listTeams -> invoking list_teams', { eventId })
  } catch (e) {}

  // FIX: backend expects 'event_id' (snake_case), not 'eventId'
  const res = await invoke<any[]>('list_teams', { eventId });

  try {
    // eslint-disable-next-line no-console
    console.debug('[api] listTeams -> response', { eventId, rows: Array.isArray(res) ? res.length : 0 })
  } catch (e) {}

  return res;
}

export const hardDeleteTeamsForEvent = (eventId: number) =>
  // FIX: backend expects 'event_id'
  invoke<void>('hard_delete_teams_for_event', { eventId });

export const createTeam = async (payload: {
  event_id: number;
  header_id: number;
  heeler_id: number;
  rating: number;
}) => {
  try {
    // eslint-disable-next-line no-console
    console.debug('[api] createTeam -> invoking create_team', payload)
  } catch (e) {}

  const res = await invoke<number>('create_team', { t: payload });

  try {
    // eslint-disable-next-line no-console
    console.debug('[api] createTeam -> response', { insertedId: res, payload })
  } catch (e) {}

  return res;
}

export const updateTeam = (payload: {
  id: number;
  rating?: number;
  status?: 'active' | 'inactive';
}) => invoke<void>('update_team', { t: payload });

export const deleteTeam = (id: number) =>
  invoke<void>('delete_team', { id });

export const getRuns = (eventId: number, round?: number) =>
  invoke<any[]>('get_runs', { eventId, round });

export const getRunsExpanded = (eventId: number, round?: number) =>
  invoke<any[]>('get_runs_expanded', { eventId, round });

export const generateDraw = (opts: {
  event_id: number;
  round: number;
  reseed?: boolean;
  seed_runs?: boolean;
}) => invoke<number>('generate_draw', { opts });

export const generateDrawBatch = (opts: {
  event_id: number;
  rounds: number;
  shuffle: boolean;
}) => invoke<number>('generate_draw_batch', { opts });

export const getStandings = (eventId: number) =>
  invoke<any[]>('get_standings', { eventId });

export const getDraw = (eventId: number, round: number) =>
  invoke<any[]>('get_draw', { eventId, round });

export const updateSeries = (id: number, patch: {
  name?: string; season?: string; status?: "active"|"upcoming"|"archived";
  start_date?: string | null; end_date?: string | null;
}) => invoke<void>('update_series', { id, patch });

export const deleteSeries = (id: number) =>
  invoke<void>('delete_series', { id });

// Events
// Events
export const getEvents = (seriesId?: number) => {
  // Only include the param when seriesId is a finite number; otherwise call without it
  // so the backend treats it as None and returns all events (used by global calendar).
  if (typeof seriesId === 'number' && Number.isFinite(seriesId)) {
    try {
      // eslint-disable-next-line no-console
      console.debug('[api] getEvents -> list_events', { seriesId })
    } catch (e) {}
    // FIX: backend expects 'series_id'
    return invoke<any[]>('list_events', { seriesId });
  }
  try {
    // eslint-disable-next-line no-console
    console.debug('[api] getEvents -> list_events (all)')
  } catch (e) {}
  return invoke<any[]>('list_events');
}

export const listAllEventsRaw = () => invoke<any[]>('list_all_events_raw');

export const createEvent = (payload: {
  series_id: number;
  name: string;
  date: string;
  rounds: number;
  status?: 'draft'|'upcoming'|'active'|'locked'|'completed'|'inactive'|'finalized';
  location?: string | null;
  entry_fee?: number | null;
  prize_pool?: number | null;
  max_team_rating?: number | null;
  payoff_allocation?: string | null;
  admin_pin?: string | null;
}) => {
  // normalize status values before sending to backend
  const p = { ...payload } as any;
  if (p.status === 'draft') p.status = 'upcoming';
  if (p.status === 'finalized') p.status = 'completed';
  // ensure we pass the payload under the same key the backend expects
  return invoke<number>('create_event', { payload: p });
}

export const updateEventStatus = (id: number, status: string) =>
  invoke<void>('update_event_status', { id, status });

export const updateEvent = (id: number, patch: {
  name?: string;
  date?: string;
  rounds?: number;
  status?: 'draft'|'upcoming'|'active'|'locked'|'completed'|'finalized'|'archived'|'inactive';
  entry_fee?: number | null;
  prize_pool?: number | null;
  location?: string | null;
  max_team_rating?: number | null;
  payoff_allocation?: string | null;
  admin_pin?: string | null;
}) => invoke<void>('update_event', { id, patch });

export const deleteEvent = (id: number) =>
  invoke<void>('delete_event', { id });

export const duplicateEvent = (id: number) =>
  invoke<number>('duplicate_event', { id });

export const saveRun = (payload: {
  event_id: number;
  team_id: number;
  round: number;
  position: number;
  time_sec: number | null;
  penalty: number;
  no_time: boolean;
  dq: boolean;
  captured_by?: number | null;
}) => invoke<number>('save_run', { payload });

// Ropers
export const listRopers = () => invoke<any[]>('list_ropers');

export const createRoper = (payload: {
  first_name: string; last_name: string;
  specialty: 'header'|'heeler'|'both';
  rating: number; phone?: string | null; email?: string | null; level?: 'pro'|'amateur'|'principiante'
}) => invoke<number>('create_roper', { r: payload });

export const updateRoper = (id: number, patch: Partial<{
  first_name: string; last_name: string;
  specialty: 'header'|'heeler'|'both';
  rating: number; phone?: string | null; email?: string | null; level?: 'pro'|'amateur'|'principiante'
}>) => invoke<void>('update_roper', { r: { id, ...patch } });

export const deleteRoper = (id: number) =>
  invoke<void>('delete_roper', { id });

// Payoffs
export const listPayoffRules = (eventId?: number) =>
  invoke<any[]>('list_payoff_rules', { eventId });

export const createPayoffRule = (rule: {
  event_id: number;
  position: number;
  percentage: number;
}) => invoke<number>('create_payoff_rule', { rule });

export const deletePayoffRule = (id: number) =>
  invoke<void>('delete_payoff_rule', { id });

export const getPayoutBreakdown = (eventId: number) =>
  invoke<{
    total_pot: number;
    deductions: number;
    net_pot: number;
    payouts: Array<{ place: number; percentage: number; amount: number }>;
  }>('get_payout_breakdown', { eventId });

export const exportEvent = (eventId: number, options: {
  overview: boolean;
  teams: boolean;
  run_order: boolean;
  standings: boolean;
  payoffs: boolean;
  event_logs: boolean;
  file_path: string;
}) => invoke<void>('export_event_to_excel', { eventId, options });

export const getRecentActivity = (limit: number, offset: number = 0) =>
  invoke<any[]>('get_recent_activity', { limit, offset });

export const getDashboardStats = () =>
  invoke<any>('get_dashboard_stats');

export const getSeriesLogs = (seriesId: number) =>
  invoke<any[]>('get_series_logs', { seriesId });
