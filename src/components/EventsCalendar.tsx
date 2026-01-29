import { useEffect, useState } from 'react'
import { Button } from './ui/button'
import { Progress } from './ui/progress'
import { getSeries, getEvents, createEvent } from '@/lib/api'
import { NewEventModal } from './NewEventModal'
// NO cambiar estilos ni estructura, solo reemplazar el contenido del placeholder con el calendario funcional.
import { Calendar, momentLocalizer } from 'react-big-calendar'
import moment from 'moment'
import 'moment/locale/es'
import 'react-big-calendar/lib/css/react-big-calendar.css'
import { toast } from 'sonner'

moment.locale('es')
const localizer = momentLocalizer(moment)

function CalendarWrapper({ events }: { events: any[] }) {
  const calendarEvents = events
    .map((e) => {
      // expect e.start / e.end or e.date
      const startStr = (e.start ?? e.start_date ?? e.date ?? e.event_date ?? null) as string | null
      const endStr = (e.end ?? e.end_date ?? startStr) as string | null
      if (!startStr) return null
      const s = String(startStr).slice(0, 10)
      const en = endStr ? String(endStr).slice(0, 10) : s
      // validate simple YYYY-MM-DD with regex to avoid Invalid Date
      const isoRe = /^\d{4}-\d{2}-\d{2}$/
      if (!isoRe.test(s) || !isoRe.test(en)) return null
      return {
        id: e.id,
        title: e.name,
        start: new Date(`${s}T00:00:00`),
        end: new Date(`${en}T23:59:59`),
        status: e.status,
        allDay: true,
      }
    })
    .filter(Boolean) as any[]

  const eventStyleGetter = (event: any) => {
    const status = String((event && (event.status || (event.event && event.event.status))) ?? '').toLowerCase()
    let backgroundColor = '#94a3b8' // draft por defecto
    if (status === 'active') backgroundColor = '#22c55e'
    else if (status === 'locked') backgroundColor = '#f97316'
    else if (status === 'completed' || status === 'finalized') backgroundColor = '#3b82f6'
    return {
      style: {
        backgroundColor,
        borderRadius: '6px',
        color: '#fff',
        border: 'none',
        display: 'block',
      },
    }
  }

  return (
    <Calendar
      localizer={localizer}
      events={calendarEvents}
      startAccessor="start"
      endAccessor="end"
      defaultView="month"
      views={["month", "week", "day"]}
      style={{ height: '100%' }}
      eventPropGetter={eventStyleGetter}
      onSelectEvent={(e: any) => toast.info(`${e.title} — ${moment(e.start).format('LL')} (${e.status})`)}
    />
  )
}

export function EventsCalendar({ onViewList }: { onViewList?: (seriesId: string) => void }) {
  const [series, setSeries] = useState<{ id: string; name: string }[]>([])
  const [selectedSeriesId, setSelectedSeriesId] = useState<string>('ALL')
  const [events, setEvents] = useState<any[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [isNewEventOpen, setIsNewEventOpen] = useState(false)

  function mapSeriesRow(row: any) {
    return { id: String(row.id), name: row.name ?? '' }
  }

  function mapEventRow(row: any) {
    const startRaw = row.start_date ?? row.date ?? row.event_date ?? null
    const endRaw = row.end_date ?? null
    const safe = (v: any) => (v === null || v === undefined ? null : String(v).slice(0, 10))
    const start = safe(startRaw)
    const end = safe(endRaw) || start
    function normStatus(s: any) {
      const v = String(s ?? 'upcoming').toLowerCase()
      if (v === 'finalized') return 'completed'
      if (v === 'upcoming') return 'draft'
      return v
    }

    return {
      id: String(row.id),
      seriesId: String(row.series_id ?? row.seriesId ?? ''),
      name: row.name ?? '',
      start,
      end,
      date: start,
      status: normStatus(row.status),
      teamsCount: Number(row.teams_count ?? 0),
      rounds: Number(row.rounds ?? 1),
      lastUpdated: '—',
      entryFee: row.entry_fee ?? undefined,
      maxTeamRating: row.max_team_rating ?? undefined,
      adminPin: row.admin_pin ?? undefined,
    }
  }

  // load series
  useEffect(() => {
    let mounted = true
    ;(async () => {
      try {
        setLoading(true)
        const data = await getSeries()
        if (!mounted) return
        setSeries((data as any[]).map(mapSeriesRow))
        setError(null)
      } catch (e: any) {
        setError(e?.toString?.() ?? 'No se pudieron cargar las series')
      } finally {
        setLoading(false)
      }
    })()
    return () => {
      mounted = false
    }
  }, [])

  // load events when selectedSeriesId changes
  useEffect(() => {
    let mounted = true
    ;(async () => {
      try {
        setLoading(true)
        // Only pass a numeric series id; otherwise request all events (global)
        let sid: number | undefined = undefined
        if (selectedSeriesId !== 'ALL') {
          const asNum = Number(selectedSeriesId)
          sid = Number.isFinite(asNum) ? asNum : undefined
        }
        const data = await getEvents(sid)
        if (!mounted) return
        try {
          // debug help: log selected series and returned count
          // eslint-disable-next-line no-console
          console.debug('[EventsCalendar] load', { selectedSeriesId, sid, returned: Array.isArray(data) ? (data as any[]).length : 0 })
        } catch (e) {}
        setEvents((data as any[]).map(mapEventRow))
        setError(null)
      } catch (e: any) {
        setError(e?.toString?.() ?? 'No se pudieron cargar los eventos')
      } finally {
        setLoading(false)
      }
    })()
    return () => {
      mounted = false
    }
  }, [selectedSeriesId])

  const total = events.length
  const countBy = (key: string) => events.filter((e) => (e.status ?? '').toLowerCase() === key).length
  const active = countBy('active')
  const locked = countBy('locked')
  const completed = countBy('completed')
  const draft = countBy('draft')
  const pct = (n: number) => (total ? Math.round((n * 100) / total) : 0)

  const handleCreateEvent = async (evt: any) => {
    const sid = selectedSeriesId === 'ALL' ? undefined : Number(selectedSeriesId)

    // validación mínima
    if (!evt?.seriesId || !evt?.name || !evt?.date) {
      toast.error('Faltan datos del evento')
      return
    }

    try {
      const sid = Number(evt.seriesId)
      if (!Number.isFinite(sid)) {
        toast.error('Series id inválido para crear evento')
        return
      }
      await createEvent({
        series_id: sid,
        name: evt.name,
        date: evt.date,
        rounds: evt.rounds || 1,
        status: evt.status ?? 'draft',
        location: null,
        entry_fee: evt.entryFee ?? null,
        prize_pool: null,
      })
      toast.success('Evento creado')
    } catch (e: any) {
      toast.error('No se pudo crear el evento')
    } finally {
      // refresh eventos de la serie actual (o todos si quisieras)
      try {
        const data = await getEvents(sid)
        setEvents((data as any[]).map(mapEventRow))
      } catch {
        // si falla el refresh, meh, al menos no truena la UI
      }
      setIsNewEventOpen(false)
    }
  }

  return (
    <div className="h-full flex overflow-hidden">
      {/* Main scrollable area */}
      <div className="flex-1 overflow-auto p-6">
        <div className="mb-6 flex items-start justify-between">
          <div>
            <h1 className="text-2xl font-semibold text-foreground">Calendario de Eventos</h1>
            <p className="text-sm text-muted-foreground">Visualiza todos los eventos activos, bloqueados o finalizados por serie.</p>
          </div>

          <div className="flex items-center gap-3">
            <select className="rounded-lg border border-border bg-card px-3 py-2 text-sm" value={selectedSeriesId} onChange={(e) => setSelectedSeriesId(e.target.value)}>
              <option value="ALL">Todas las Series</option>
              {series.map((s) => (
                <option key={s.id} value={s.id}>{s.name}</option>
              ))}
            </select>

            <Button variant="outline" className="rounded-xl" onClick={() => onViewList?.(selectedSeriesId)}>
              Ver Lista
            </Button>
            <Button
              className="bg-primary text-primary-foreground rounded-xl"
              onClick={() => {
                if (selectedSeriesId === 'ALL') {
                  toast.info('Selecciona una serie para crear un evento')
                  return
                }
                setIsNewEventOpen(true)
              }}
              disabled={selectedSeriesId === 'ALL'}
            >
              + Nuevo Evento
            </Button>
          </div>
        </div>

        {loading && <div className="mb-2 text-sm text-muted-foreground">Cargando…</div>}
        {error && <div className="mb-2 rounded-md bg-red-50 p-2 text-sm text-red-700">{error}</div>}

        <div className="bg-card border border-border rounded-xl p-4 shadow-sm">
          {/* Placeholder del calendario: aquí reemplazar por un calendario real en futuras iteraciones */}
          <div className="h-[520px] rounded-md border border-gray-200 bg-white overflow-hidden">
            {/* NO cambiar estilos ni estructura, solo reemplazar el contenido del placeholder con el calendario funcional. */}
            <CalendarWrapper events={events} />
          </div>

          <div className="mt-6 flex justify-center gap-6">
            <div className="flex items-center gap-2"><span className="h-3 w-3 rounded-full bg-green-500" /> <span className="text-sm text-muted-foreground">Activo</span></div>
            <div className="flex items-center gap-2"><span className="h-3 w-3 rounded-full bg-orange-500" /> <span className="text-sm text-muted-foreground">Locked</span></div>
            <div className="flex items-center gap-2"><span className="h-3 w-3 rounded-full bg-blue-500" /> <span className="text-sm text-muted-foreground">Finalized</span></div>
            <div className="flex items-center gap-2"><span className="h-3 w-3 rounded-full bg-slate-400" /> <span className="text-sm text-muted-foreground">Draft</span></div>
          </div>

          {/* Próximos eventos: lista simple debajo del calendario para acceso rápido */}
          <div className="mt-6">
            <h4 className="text-sm text-muted-foreground mb-3">Próximos eventos</h4>
            <div className="space-y-2">
              {(() => {
                const now = new Date()
                const upcoming = events
                  .map((e) => ({ ...e, parsedDate: e.start ? new Date(`${String(e.start).slice(0,10)}T00:00:00`) : null }))
                  .filter((e) => e.parsedDate >= new Date(now.getFullYear(), now.getMonth(), now.getDate()))
                  .sort((a, b) => a.parsedDate.getTime() - b.parsedDate.getTime())
                  .slice(0, 5)

                if (upcoming.length === 0) {
                  return <div className="text-sm text-muted-foreground">No hay eventos próximos.</div>
                }

                return upcoming.map((ev) => (
                  <button key={ev.id} onClick={() => toast(`${ev.name} — ${moment(ev.parsedDate).format('LL')} (${ev.status})`)} className="w-full text-left rounded-md hover:bg-accent/50 p-3 border border-border bg-card flex items-center justify-between">
                    <div>
                      <div className="font-medium">{ev.name}</div>
                      <div className="text-xs text-muted-foreground">{moment(ev.parsedDate).format('LL')}</div>
                    </div>
                    <div className="ml-4">
                      <span
                        className={`px-2 py-1 rounded-full text-xs font-semibold ${
                          ev.status.toLowerCase() === 'active'
                            ? 'bg-green-500 text-white'
                            : ev.status.toLowerCase() === 'locked'
                            ? 'bg-orange-500 text-white'
                            : ev.status.toLowerCase() === 'completed' || ev.status.toLowerCase() === 'finalized'
                            ? 'bg-blue-500 text-white'
                            : 'bg-slate-400 text-white'
                        }`}
                      >
                        {ev.status}
                      </span>
                    </div>
                  </button>
                ))
              })()}
            </div>
          </div>
        </div>
      </div>

      {/* Right metrics sidebar: fixed/sticky to viewport */}
      <aside className="w-80 hidden lg:block border-l border-border bg-background/50 p-6 sticky top-0 h-screen">
        <h3 className="text-sm text-muted-foreground mb-4">Métricas</h3>

        <div className="bg-card rounded-md p-4 mb-4 border border-border">
          <div className="text-xs text-muted-foreground">Total Eventos</div>
          <div className="text-2xl font-semibold mt-2">{total}</div>
        </div>

        <div className="space-y-3 mb-4">
          <div className="bg-green-50 border border-green-100 rounded-md p-3 flex items-center justify-between">
            <div className="flex items-center gap-3">
              <span className="h-3 w-3 rounded-full bg-green-500" />
              <div>
                <div className="text-sm">Activos</div>
                <div className="text-xs text-muted-foreground">{active}</div>
              </div>
            </div>
          </div>

          <div className="bg-orange-50 border border-orange-100 rounded-md p-3 flex items-center justify-between">
            <div className="flex items-center gap-3">
              <span className="h-3 w-3 rounded-full bg-orange-500" />
              <div>
                <div className="text-sm">Locked</div>
                <div className="text-xs text-muted-foreground">{locked}</div>
              </div>
            </div>
          </div>

          <div className="bg-blue-50 border border-blue-100 rounded-md p-3 flex items-center justify-between">
            <div className="flex items-center gap-3">
              <span className="h-3 w-3 rounded-full bg-blue-500" />
              <div>
                <div className="text-sm">Finalizados</div>
                <div className="text-xs text-muted-foreground">{completed}</div>
              </div>
            </div>
          </div>

          <div className="bg-slate-50 border border-slate-100 rounded-md p-3 flex items-center justify-between">
            <div className="flex items-center gap-3">
              <span className="h-3 w-3 rounded-full bg-slate-400" />
              <div>
                <div className="text-sm">Draft</div>
                <div className="text-xs text-muted-foreground">{draft}</div>
              </div>
            </div>
          </div>
        </div>

        <div className="bg-card rounded-md p-4 border border-border mb-4">
          <div className="text-sm text-muted-foreground mb-2">Distribución</div>

          <div className="text-xs text-muted-foreground">Activos <span className="float-right">{pct(active)}%</span></div>
          <Progress value={pct(active)} className="mt-2 mb-3" />

          <div className="text-xs text-muted-foreground">Locked <span className="float-right">{pct(locked)}%</span></div>
          <Progress value={pct(locked)} className="mt-2 mb-3" />

          <div className="text-xs text-muted-foreground">Finalizados <span className="float-right">{pct(completed)}%</span></div>
          <Progress value={pct(completed)} className="mt-2" />
        </div>

        <Button variant="outline" className="w-full rounded-lg">Ver Logs de Eventos</Button>
      </aside>

      {/* NewEventModal: controlled locally */}
      <NewEventModal isOpen={isNewEventOpen} onClose={() => setIsNewEventOpen(false)} seriesId={selectedSeriesId === 'ALL' ? '' : selectedSeriesId} onCreateEvent={async (e) => await handleCreateEvent(e)} />
    </div>
  )
}
