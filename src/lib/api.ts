import { invoke } from '@tauri-apps/api/core';
import type {
  LicenseInputPayload,
  LicensePlan,
  LicenseRequestSummaryDto,
  LicenseStatusDto,
} from '../types/license';

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

  const res = await invoke<any[]>('list_teams', { eventId });

  try {
    // eslint-disable-next-line no-console
    console.debug('[api] listTeams -> response', { eventId, rows: Array.isArray(res) ? res.length : 0 })
  } catch (e) {}

  return res;
}

export const hardDeleteTeamsForEvent = (eventId: number) =>
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

export const getSeriesResultsSummary = (seriesId: number) =>
  invoke<any>('get_series_results_summary', { seriesId });

export const getSeriesRoperRankings = (seriesId: number) =>
  invoke<any[]>('get_series_roper_rankings', { seriesId });

export const getSeriesRoperProfile = (seriesId: number, roperId: number) =>
  invoke<any | null>('get_series_roper_profile', { seriesId, roperId });

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
export const listRopers = (options?: { includeInactive?: boolean }) => {
  const payload = options?.includeInactive ? { include_inactive: true } : {};
  return invoke<any[]>('list_ropers', payload);
};

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

export const deleteAllRopers = () =>
  invoke<number>('delete_all_ropers');

// Event Roster
export type EventRosterSyncEntry = {
  external_id?: string | null;
  first_name: string;
  last_name: string;
  specialty?: 'header'|'heeler'|'both';
  rating?: number;
  phone?: string | null;
  normalized_phone?: string | null;
  email?: string | null;
  level?: 'pro'|'amateur'|'principiante';
  status?: 'registered'|'confirmed'|'withdrawn';
  rating_override?: number | null;
  notes?: string | null;
  source_hash?: string | null;
};

export type SyncEventRosterResult = {
  created_ropers: number;
  updated_ropers: number;
  reactivated_ropers: number;
  roster_upserts: number;
  roster_marked_withdrawn: number;
};

export const listEventRoster = (eventId: number, options?: { includeWithdrawn?: boolean }) =>
  invoke<any[]>('list_event_roster', {
    eventId,
    event_id: eventId,
    includeWithdrawn: options?.includeWithdrawn ?? false,
    include_withdrawn: options?.includeWithdrawn ?? false,
  });

export const updateEventRosterEntry = (payload: {
  id: number;
  status?: 'registered'|'confirmed'|'withdrawn';
  rating_override?: number | null;
  notes?: string | null;
}) => invoke<void>('update_event_roster_entry', { payload });

export const syncEventRoster = (payload: {
  event_id: number;
  entries: EventRosterSyncEntry[];
  withdraw_absent?: boolean;
}) => invoke<SyncEventRosterResult>('sync_event_roster', { payload });

// Payoffs
export const listPayoffRules = (eventId?: number) => {
  if (typeof eventId === 'number' && Number.isFinite(eventId)) {
    return invoke<any[]>('list_payoff_rules', { eventId });
  }
  return invoke<any[]>('list_payoff_rules');
};

// Licensing
export const getDeviceHash = () => invoke<string>('get_device_hash');

export const generateLicenseRequest = (
  plan: LicensePlan,
  customerNameHint?: string,
  destinationPath?: string,
) =>
  invoke<LicenseRequestSummaryDto>('generate_license_request', {
    plan,
    customerNameHint,
    destinationPath,
  });

export const installLicense = (input: LicenseInputPayload) =>
  invoke<LicenseStatusDto>('install_license', { input });

export const getLicenseStatus = () =>
  invoke<LicenseStatusDto | null>('license_status');

export const removeLicense = () => invoke<void>('remove_license');

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
  include_blocked?: boolean;
  file_path: string;
}) => invoke<void>('export_event_to_excel', { eventId, options });

export const getRecentActivity = (limit: number, offset: number = 0) =>
  invoke<any[]>('get_recent_activity', { limit, offset });

export const getDashboardStats = () =>
  invoke<any>('get_dashboard_stats');

export const getSeriesLogs = (seriesId: number, limit = 50) =>
  invoke<any[]>('get_series_logs', { seriesId, limit });

// Timer Capture
export interface SerialPortInfo {
  port_name: string;
  port_type: string;
}

export interface TimerEvent {
  time_seconds: number;
  raw_text: string;
  timestamp: string;
}

export const listSerialPorts = () =>
  invoke<SerialPortInfo[]>('list_serial_ports');

export const connectTimer = (portName: string) =>
  invoke<void>('connect_timer', { portName });

export const disconnectTimer = () =>
  invoke<void>('disconnect_timer');

export const isTimerConnected = () =>
  invoke<boolean>('is_timer_connected');

export const startTimerCapture = () =>
  invoke<void>('start_timer_capture');
