import { SeriesKpiGrid } from './SeriesKpiGrid'
import { SeriesRopersFilters } from './SeriesRopersFilters'
import { SeriesRopersTable } from './SeriesRopersTable'
import type {
  SeriesRoperStatRow,
  SeriesRopersFiltersState,
  SeriesRopersSummary,
} from './types'

interface SeriesRopersViewProps {
  rows: SeriesRoperStatRow[]
  summary: SeriesRopersSummary
  filters: SeriesRopersFiltersState
  onFiltersChange: (patch: Partial<SeriesRopersFiltersState>) => void
  onSelectRoper: (roperId: number) => void
}

export function SeriesRopersView({
  rows,
  summary,
  filters,
  onFiltersChange,
  onSelectRoper,
}: SeriesRopersViewProps) {
  return (
    <>
      <div className="mb-4 flex items-center justify-between rounded-xl border border-border bg-card px-4 py-3">
        <div>
          <h2 className="font-medium text-foreground">Serie / Ropers</h2>
          <p className="text-sm text-muted-foreground">
            Ranking global ordenado por tiempo promedio.
          </p>
        </div>
      </div>

      <SeriesKpiGrid summary={summary} />
      <SeriesRopersFilters value={filters} onChange={onFiltersChange} />
      <SeriesRopersTable rows={rows} onSelectRoper={onSelectRoper} />
    </>
  )
}
