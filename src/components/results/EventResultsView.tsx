import { EventKpiGrid } from './EventKpiGrid'
import { EventPodium } from './EventPodium'
import { EventResultsFilters } from './EventResultsFilters'
import { EventResultsTable } from './EventResultsTable'
import type {
  EventResultsFiltersState,
  EventResultsSummary,
  EventStandingRow,
} from './types'

interface EventResultsViewProps {
  rows: EventStandingRow[]
  summary: EventResultsSummary
  filters: EventResultsFiltersState
  onFiltersChange: (patch: Partial<EventResultsFiltersState>) => void
  onSelectTeam: (teamId: number) => void
}

export function EventResultsView({
  rows,
  summary,
  filters,
  onFiltersChange,
  onSelectTeam,
}: EventResultsViewProps) {
  return (
    <>
      {rows.length > 0 ? (
        <EventPodium first={rows[0]} second={rows[1]} third={rows[2]} />
      ) : (
        <div className="mb-6 rounded-xl border border-dashed border-border p-12 text-center">
          <p className="text-muted-foreground">Selecciona un evento para ver los resultados</p>
        </div>
      )}

      <EventKpiGrid summary={summary} />
      <EventResultsFilters value={filters} onChange={onFiltersChange} />
      <EventResultsTable rows={rows} onSelectTeam={onSelectTeam} />
    </>
  )
}
