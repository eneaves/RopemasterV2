import { Button } from '../ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../ui/select'
import type { Event, Series } from '../../types'
import { ResultsViewToggle } from './ResultsViewToggle'
import type { ResultsView } from './types'

interface ResultsHeaderProps {
  seriesList: Series[]
  eventsList: Event[]
  selectedSeriesId: string | null
  selectedEventId: string | null
  selectedSeries?: Series
  selectedEvent?: Event
  activeView: ResultsView
  onSeriesChange: (value: string) => void
  onEventChange: (value: string) => void
  onViewChange: (value: ResultsView) => void
  onRefresh: () => void
  onExport: () => void
}

export function ResultsHeader({
  seriesList,
  eventsList,
  selectedSeriesId,
  selectedEventId,
  selectedSeries,
  selectedEvent,
  activeView,
  onSeriesChange,
  onEventChange,
  onViewChange,
  onRefresh,
  onExport,
}: ResultsHeaderProps) {
  return (
    <div className="mb-6 flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
      <div>
        <h1 className="text-2xl font-semibold text-foreground">Resultados</h1>
        <p className="text-sm text-muted-foreground">
          {selectedSeries ? `Serie: ${selectedSeries.name}` : 'Selecciona una serie'} —
          {selectedEvent ? ` Evento: ${selectedEvent.name}` : ' Selecciona un evento'}
        </p>
      </div>

      <div className="flex flex-wrap items-center gap-3 xl:justify-end">
        <Select value={selectedSeriesId || ''} onValueChange={onSeriesChange}>
          <SelectTrigger className="w-[180px] rounded-lg border border-border bg-card px-3 py-2 text-sm">
            <SelectValue placeholder="Serie" />
          </SelectTrigger>
          <SelectContent>
            {seriesList.map((series) => (
              <SelectItem key={series.id} value={series.id.toString()}>
                {series.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Select
          value={selectedEventId || ''}
          onValueChange={onEventChange}
          disabled={!selectedSeriesId}
        >
          <SelectTrigger className="w-[180px] rounded-lg border border-border bg-card px-3 py-2 text-sm">
            <SelectValue placeholder="Evento" />
          </SelectTrigger>
          <SelectContent>
            {eventsList.map((event) => (
              <SelectItem key={event.id} value={event.id.toString()}>
                {event.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <ResultsViewToggle value={activeView} onChange={onViewChange} />

        <Button variant="ghost" onClick={onRefresh}>
          Refrescar
        </Button>
        <Button className="bg-orange-500 text-white hover:bg-orange-600" onClick={onExport}>
          Exportar Resultados
        </Button>
      </div>
    </div>
  )
}
