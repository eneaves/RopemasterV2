import { useEffect, useState, useCallback } from 'react';
import { listTeams, createTeam, updateTeam, deleteTeam } from '@/lib/api';

export function useTeams(eventId: number, locked: boolean) {
  const [teams, setTeams] = useState<any[]>([]);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    // require a positive numeric eventId
    if (typeof eventId !== 'number' || Number.isNaN(eventId) || eventId <= 0) return;
    try {
      setLoading(true);
      setErr(null);
      console.log(`Fetching teams for event ${eventId}...`);
      const data = await listTeams(eventId);
      console.log('Raw teams data:', data);

      // DEBUG: raw rows from backend
      try {
        // eslint-disable-next-line no-console
        console.debug('[useTeams] refresh (raw rows)', { eventId, rows: Array.isArray(data) ? data : [] });
      } catch (e) {}

      // Normalize rows to expected shape
      const normalized = (Array.isArray(data) ? data : []).map((r: any) => ({
        id: Number(r.id),
        event_id: Number(r.event_id ?? r.eventId ?? 0),
        header_id: Number(r.header_id ?? r.headerId ?? r.header ?? 0),
        heeler_id: Number(r.heeler_id ?? r.heelerId ?? r.heeler ?? 0),
        rating: Number(r.rating ?? r.team_rating ?? 0),
        status: r.status ?? 'active',
        created_at: r.created_at,
        updated_at: r.updated_at,
      }));

      try {
        // eslint-disable-next-line no-console
        console.debug('[useTeams] refresh (normalized)', { eventId, normalizedCount: normalized.length, sample: normalized.slice(0,3) });
      } catch (e) {}

      setTeams(normalized as any[]);
      return normalized as any[];
    } catch (e: any) {
      setErr(e?.toString?.() ?? 'Error cargando equipos');
      return [] as any[];
    } finally {
      setLoading(false);
    }
  }, [eventId]);

  useEffect(() => {
    if (eventId) refresh();
  }, [eventId, refresh]);

  const add = async (
    payload: { header_id: number; heeler_id: number; rating: number },
    options?: { suppressRefresh?: boolean }
  ) => {
    if (locked) throw new Error('Evento bloqueado; no puedes crear equipos.');
    // createTeam expects event_id in payload; include the current eventId
    try {
      const createdId = await createTeam({ event_id: eventId, ...payload } as any);
      if (!options?.suppressRefresh) {
        await refresh();
      }
      return createdId as number
    } catch (e: any) {
      const message = e?.toString?.() ?? 'Error creando equipo';
      setErr(message);
      // rethrow so callers (UI flow) can handle and show toasts
      throw new Error(message);
    }
  };

  const edit = async (payload: { id: number; rating?: number; status?: 'active' | 'inactive' }) => {
    if (locked) throw new Error('Evento bloqueado; no puedes editar equipos.');
    await updateTeam(payload as any);
    await refresh();
  };

  const remove = async (id: number) => {
    if (locked) throw new Error('Evento bloqueado; no puedes borrar equipos.');
    await deleteTeam(id);
    await refresh();
  };

  return { teams, loading, err, refresh, add, edit, remove };
}
