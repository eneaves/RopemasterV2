import { useEffect, useMemo, useState } from 'react'
import {
  exportEvent,
  getEvents,
  getPayoutBreakdown,
  getSeries,
  getSeriesResultsSummary,
  getSeriesRoperProfile,
  getSeriesRoperRankings,
  getStandings,
} from '../lib/api'
import { save } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'
import type { Event, Series } from '../types'
import { EventResultsView } from './results/EventResultsView'
import { ResultsHeader } from './results/ResultsHeader'
import { RoperProfileDialog } from './results/RoperProfileDialog'
import { buildEventResultsSummary } from './results/mock'
import { SeriesRopersView } from './results/SeriesRopersView'
import { SeriesSummaryPanel } from './results/SeriesSummaryPanel'
import { TeamResultDialog } from './results/TeamResultDialog'
import type {
  EventResultsFiltersState,
  EventStandingRow,
  ResultsView,
  SeriesRoperProfile,
  SeriesRoperStatRow,
  SeriesRopersFiltersState,
  SeriesRopersSummary,
  SeriesSummaryPanelData,
} from './results/types'

function mapSeriesSummaryDto(dto: any): {
  summary: SeriesRopersSummary
  panel: SeriesSummaryPanelData
} {
  return {
    summary: {
      uniqueRopers: Number(dto?.unique_ropers ?? 0),
      closedEvents: Number(dto?.closed_events ?? 0),
      validRuns: Number(dto?.valid_runs ?? 0),
      totalDistributed: Number(dto?.total_distributed ?? 0),
      fastestRoperName: dto?.fastest_roper_name ?? null,
      fastestAvgTime: dto?.fastest_avg_time ?? null,
      mostWinsRoperName: dto?.most_wins_roper_name ?? null,
      mostWinsCount: Number(dto?.most_wins_count ?? 0),
    },
    panel: {
      closedEvents: Number(dto?.closed_events ?? 0),
      uniqueRopers: Number(dto?.unique_ropers ?? 0),
      teamsRegistered: Number(dto?.teams_registered ?? 0),
      totalDistributed: Number(dto?.total_distributed ?? 0),
      avgSeriesTime: dto?.avg_series_time ?? null,
      cleanRunRate: dto?.clean_run_rate ?? null,
      topRopers: Array.isArray(dto?.top_ropers)
        ? dto.top_ropers.map((row: any) => ({
            roperId: Number(row.roper_id),
            name: row.name,
            avgTime: row.avg_time ?? null,
          }))
        : [],
    },
  }
}

function mapSeriesRankingDto(dto: any): SeriesRoperStatRow {
  return {
    roperId: Number(dto.roper_id),
    roperName: dto.roper_name,
    specialty: dto.specialty,
    eventsPlayed: Number(dto.events_played ?? 0),
    partnersCount: Number(dto.partners_count ?? 0),
    validRuns: Number(dto.valid_runs ?? 0),
    avgTime: dto.avg_time ?? null,
    bestRun: dto.best_run ?? null,
    wins: Number(dto.wins ?? 0),
    podiums: Number(dto.podiums ?? 0),
    ntCount: Number(dto.nt_count ?? 0),
    dqCount: Number(dto.dq_count ?? 0),
    earnings: Number(dto.earnings ?? 0),
    rank: Number(dto.rank ?? 0),
  }
}

function mapSeriesProfileDto(dto: any): SeriesRoperProfile | null {
  if (!dto) return null

  return {
    roperId: Number(dto.roper_id),
    roperName: dto.roper_name,
    specialty: dto.specialty,
    rank: Number(dto.rank ?? 0),
    avgTime: dto.avg_time ?? null,
    eventsPlayed: Number(dto.events_played ?? 0),
    wins: Number(dto.wins ?? 0),
    podiums: Number(dto.podiums ?? 0),
    earnings: Number(dto.earnings ?? 0),
    bestPartnerName: dto.best_partner_name ?? null,
    bestEventName: dto.best_event_name ?? null,
    bestRun: dto.best_run ?? null,
    history: Array.isArray(dto?.history)
      ? dto.history.map((entry: any) => ({
          eventId: Number(entry.event_id),
          eventName: entry.event_name,
          partnerName: entry.partner_name,
          finishRank: entry.finish_rank ?? null,
          totalTime: entry.total_time ?? null,
          avgTime: entry.avg_time ?? null,
          earnings: Number(entry.earnings ?? 0),
        }))
      : [],
  }
}

export function ResultsManagement() {
  const [seriesList, setSeriesList] = useState<Series[]>([])
  const [eventsList, setEventsList] = useState<Event[]>([])
  const [selectedSeriesId, setSelectedSeriesId] = useState<string | null>(null)
  const [selectedEventId, setSelectedEventId] = useState<string | null>(null)
  const [activeView, setActiveView] = useState<ResultsView>('event')
  const [standings, setStandings] = useState<EventStandingRow[]>([])
  const [eventFilters, setEventFilters] = useState<EventResultsFiltersState>({
    query: '',
    place: 'Todos',
    status: 'Todos',
  })
  const [seriesFilters, setSeriesFilters] = useState<SeriesRopersFiltersState>({
    query: '',
    specialty: 'Todos',
    minRuns: '3',
    scope: 'Todos',
  })
  const [seriesSummary, setSeriesSummary] = useState<SeriesRopersSummary | null>(null)
  const [seriesPanelSummary, setSeriesPanelSummary] = useState<SeriesSummaryPanelData | null>(null)
  const [seriesRoperRows, setSeriesRoperRows] = useState<SeriesRoperStatRow[]>([])
  const [selectedTeamId, setSelectedTeamId] = useState<number | null>(null)
  const [selectedRoperId, setSelectedRoperId] = useState<number | null>(null)
  const [selectedRoperProfile, setSelectedRoperProfile] = useState<SeriesRoperProfile | null>(null)

  useEffect(() => {
    getSeries().then((data) => {
      setSeriesList(data)
      const active = data.find((series) => series.status === 'active')
      if (active) setSelectedSeriesId(active.id.toString())
    })
  }, [])

  useEffect(() => {
    if (selectedSeriesId) {
      getEvents(parseInt(selectedSeriesId, 10)).then((data) => {
        setEventsList(data)
        const active = data.find((event) => event.status === 'active' || event.status === 'completed')
        if (active) setSelectedEventId(active.id.toString())
        else setSelectedEventId(null)
      })
    } else {
      setEventsList([])
    }
  }, [selectedSeriesId])

  useEffect(() => {
    if (selectedEventId) {
      Promise.all([
        getStandings(parseInt(selectedEventId, 10)),
        getPayoutBreakdown(parseInt(selectedEventId, 10)),
      ])
        .then(([standingsData, payoutData]) => {
          const payoutMap = new Map<number, number>()
          payoutData.payouts.forEach((payout: any) => payoutMap.set(payout.place, payout.amount))

          const mapped: EventStandingRow[] = standingsData.map((row: any) => {
            let status: EventStandingRow['status'] = 'Calificado'
            if (row.dq_cnt > 0) status = 'DQ'
            else if (row.nt_cnt > 0) status = 'No Time'

            return {
              rank: row.rank,
              teamId: row.team_id,
              headerName: row.header_name,
              heelerName: row.heeler_name,
              totalTime: row.total_time,
              completedRuns: row.completed_runs,
              ntCount: row.nt_cnt,
              dqCount: row.dq_cnt,
              avgTime: row.avg_time,
              bestTime: row.best_time,
              payoff: payoutMap.get(row.rank) || 0,
              status,
            }
          })

          setStandings(mapped)
        })
        .catch((error) => {
          console.error(error)
          toast.error('Error al cargar resultados')
        })
    } else {
      setStandings([])
    }
  }, [selectedEventId])

  useEffect(() => {
    if (!selectedSeriesId) {
      setSeriesSummary(null)
      setSeriesPanelSummary(null)
      setSeriesRoperRows([])
      setSelectedRoperId(null)
      setSelectedRoperProfile(null)
      return
    }

    let cancelled = false
    setSeriesSummary(null)
    setSeriesPanelSummary(null)

    Promise.all([
      getSeriesResultsSummary(parseInt(selectedSeriesId, 10)),
      getSeriesRoperRankings(parseInt(selectedSeriesId, 10)),
    ])
      .then(([summaryDto, rankingsDto]) => {
        if (cancelled) return
        const mappedSummary = mapSeriesSummaryDto(summaryDto)
        setSeriesSummary(mappedSummary.summary)
        setSeriesPanelSummary(mappedSummary.panel)
        setSeriesRoperRows(Array.isArray(rankingsDto) ? rankingsDto.map(mapSeriesRankingDto) : [])
      })
      .catch((error) => {
        console.error(error)
        if (cancelled) return
        setSeriesSummary(null)
        setSeriesPanelSummary(null)
        setSeriesRoperRows([])
        toast.error('Error al cargar estadísticas de la serie')
      })

    return () => {
      cancelled = true
    }
  }, [selectedSeriesId])

  useEffect(() => {
    if (!selectedSeriesId || selectedRoperId === null) {
      setSelectedRoperProfile(null)
      return
    }

    let cancelled = false
    setSelectedRoperProfile(null)

    getSeriesRoperProfile(parseInt(selectedSeriesId, 10), selectedRoperId)
      .then((profileDto) => {
        if (cancelled) return
        setSelectedRoperProfile(mapSeriesProfileDto(profileDto))
      })
      .catch((error) => {
        console.error(error)
        if (cancelled) return
        setSelectedRoperProfile(null)
        toast.error('Error al cargar perfil del roper')
      })

    return () => {
      cancelled = true
    }
  }, [selectedRoperId, selectedSeriesId])

  const selectedSeries = seriesList.find((series) => series.id.toString() === selectedSeriesId)
  const selectedEvent = eventsList.find((event) => event.id.toString() === selectedEventId)
  const eventSummary = useMemo(() => buildEventResultsSummary(standings), [standings])

  const filteredEventRows = useMemo(() => {
    return standings.filter((row) => {
      const search = `${row.headerName} ${row.heelerName}`.toLowerCase()
      if (eventFilters.query && !search.includes(eventFilters.query.toLowerCase())) return false
      if (eventFilters.place !== 'Todos' && String(row.rank) !== eventFilters.place) return false
      if (eventFilters.status !== 'Todos' && row.status !== eventFilters.status) return false
      return true
    })
  }, [eventFilters, standings])

  const filteredSeriesRows = useMemo(() => {
    return seriesRoperRows.filter((row) => {
      if (seriesFilters.query && !row.roperName.toLowerCase().includes(seriesFilters.query.toLowerCase())) {
        return false
      }
      if (seriesFilters.specialty !== 'Todos' && row.specialty !== seriesFilters.specialty) {
        return false
      }
      if (row.validRuns < Number(seriesFilters.minRuns)) return false
      if (seriesFilters.scope === 'Top 10' && row.rank > 10) return false
      if (seriesFilters.scope === 'Con podio' && row.podiums === 0) return false
      return true
    })
  }, [seriesFilters, seriesRoperRows])

  const selectedTeam = useMemo(
    () => standings.find((row) => row.teamId === selectedTeamId) ?? null,
    [selectedTeamId, standings],
  )

  const handleExport = async () => {
    if (!selectedEventId) return
    try {
      const filePath = await save({
        filters: [{ name: 'Excel', extensions: ['xlsx'] }],
      })
      if (filePath) {
        await exportEvent(parseInt(selectedEventId, 10), {
          overview: true,
          teams: true,
          run_order: true,
          standings: true,
          payoffs: true,
          event_logs: true,
          file_path: filePath,
        })
        toast.success('Resultados exportados')
      }
    } catch (error) {
      console.error(error)
      toast.error('Error al exportar')
    }
  }

  const handleRefresh = () => {
    const current = selectedEventId
    setSelectedEventId(null)
    setTimeout(() => setSelectedEventId(current), 50)
  }

  return (
    <div className="flex flex-1 overflow-hidden bg-background">
      <div className="flex-1 overflow-y-auto">
        <div className="max-w-full p-6">
          <ResultsHeader
            seriesList={seriesList}
            eventsList={eventsList}
            selectedSeriesId={selectedSeriesId}
            selectedEventId={selectedEventId}
            selectedSeries={selectedSeries}
            selectedEvent={selectedEvent}
            activeView={activeView}
            onSeriesChange={setSelectedSeriesId}
            onEventChange={setSelectedEventId}
            onViewChange={setActiveView}
            onRefresh={handleRefresh}
            onExport={handleExport}
          />

          {activeView === 'event' ? (
            <EventResultsView
              rows={filteredEventRows}
              summary={eventSummary}
              filters={eventFilters}
              onFiltersChange={(patch) => setEventFilters((current) => ({ ...current, ...patch }))}
              onSelectTeam={setSelectedTeamId}
            />
          ) : (
            <SeriesRopersView
              rows={filteredSeriesRows}
              summary={
                seriesSummary ?? {
                  uniqueRopers: 0,
                  closedEvents: 0,
                  validRuns: 0,
                  totalDistributed: 0,
                  fastestRoperName: null,
                  fastestAvgTime: null,
                  mostWinsRoperName: null,
                  mostWinsCount: 0,
                }
              }
              filters={seriesFilters}
              onFiltersChange={(patch) => setSeriesFilters((current) => ({ ...current, ...patch }))}
              onSelectRoper={setSelectedRoperId}
            />
          )}
        </div>
      </div>

      <SeriesSummaryPanel summary={seriesPanelSummary} onSelectRoper={setSelectedRoperId} />

      <TeamResultDialog
        open={selectedTeam !== null}
        eventId={selectedEventId ? parseInt(selectedEventId, 10) : null}
        team={selectedTeam}
        onOpenChange={(open) => {
          if (!open) setSelectedTeamId(null)
        }}
      />

      <RoperProfileDialog
        open={selectedRoperId !== null}
        profile={selectedRoperProfile}
        onOpenChange={(open) => {
          if (!open) {
            setSelectedRoperId(null)
            setSelectedRoperProfile(null)
          }
        }}
      />
    </div>
  )
}
